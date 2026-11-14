// SPDX-License-Identifier: Apache-2.0
// SPDX-FileCopyrightText: 2026 The Gleam contributors

//! Static checking of `requires` / `ensures` contracts with an SMT solver.
//!
//! Verification is modular: a call is checked against the callee's contract,
//! never its body, so each function is one independent set of queries.
//! Anything the encoder does not understand becomes an unconstrained value,
//! which can only make a proof fail, never succeed wrongly. Contracts that are
//! not proved stay in the generated code as runtime asserts.

use std::collections::HashMap;
use std::io::Write;
use std::process::{Command, Stdio};

use ecow::{EcoString, eco_format};

use crate::analyse::CONTRACT_RESULT;
use crate::ast::{
    AssertKind, AssignmentKind, BinOp, Pattern, Statement, TypedExpr, TypedFunction, TypedModule,
    TypedStatement,
};
use crate::type_::{Type, ValueConstructorVariant};
use src_span::SrcSpan;
use std::sync::Arc;

const TIMEOUT_MS: u32 = 5000;

/// Gleam integer division and remainder on Erlang: truncate toward zero and
/// return 0 when dividing by zero. SMT-LIB `div`/`mod` are Euclidean, so using
/// them directly proves things that are false at runtime.
const PREAMBLE: &str = "\
(set-option :timeout 5000)
(define-fun gdiv ((a Int) (b Int)) Int
  (ite (= b 0) 0
    (ite (>= a 0)
      (ite (> b 0) (div a b) (- (div a (- b))))
      (ite (> b 0) (- (div (- a) b)) (div (- a) (- b))))))
(define-fun grem ((a Int) (b Int)) Int
  (ite (= b 0) 0 (- a (* b (gdiv a b)))))
";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Obligation {
    Ensures,
    CallRequires { callee: EcoString },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Proved,
    Counterexample(Vec<(EcoString, String)>),
    Unknown(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub function: EcoString,
    pub location: SrcSpan,
    pub obligation: Obligation,
    pub outcome: Outcome,
}

#[derive(Debug)]
pub enum Error {
    SolverUnavailable(String),
}

struct Contract<'a> {
    parameters: Vec<Option<EcoString>>,
    requires: Vec<&'a TypedExpr>,
    ensures: Vec<&'a TypedExpr>,
}

pub fn has_contracts(module: &TypedModule) -> bool {
    module
        .definitions
        .functions
        .iter()
        .any(|function| contract_of(function).is_some())
}

pub fn verify_module(module: &TypedModule) -> Result<Vec<Finding>, Error> {
    let contracts: HashMap<EcoString, Contract<'_>> = module
        .definitions
        .functions
        .iter()
        .filter_map(|function| {
            let name = function.name.as_ref()?.1.clone();
            Some((name, contract_of(function)?))
        })
        .collect();
    if contracts.is_empty() {
        return Ok(vec![]);
    }

    let mut findings = vec![];
    for function in &module.definitions.functions {
        let Some((_, name)) = &function.name else {
            continue;
        };
        let mut encoder = Encoder::new(&module.name, &contracts);
        encoder.function(function);
        for query in encoder.obligations {
            let outcome = solve(
                &encoder.declarations,
                &encoder.facts,
                &query,
                &encoder.inputs,
            )?;
            findings.push(Finding {
                function: name.clone(),
                location: query.location,
                obligation: query.obligation,
                outcome,
            });
        }
    }
    Ok(findings)
}

/// Contract clauses are the asserts that `analyse::desugar_contracts` put at
/// the top level of the body.
fn contract_of(function: &TypedFunction) -> Option<Contract<'_>> {
    let mut requires = vec![];
    let mut ensures = vec![];
    for statement in &function.body {
        if let Statement::Assert(assert) = statement {
            match assert.kind {
                AssertKind::Requires => requires.push(&assert.value),
                AssertKind::Ensures => ensures.push(&assert.value),
                AssertKind::Plain => {}
            }
        }
    }
    if requires.is_empty() && ensures.is_empty() {
        return None;
    }
    Some(Contract {
        parameters: function
            .arguments
            .iter()
            .map(|argument| argument.names.get_variable_name().cloned())
            .collect(),
        requires,
        ensures,
    })
}

struct Query {
    /// Only facts recorded before the obligation may be used to prove it;
    /// later ones include the obligation itself, assumed once checked.
    facts: usize,
    path: Vec<String>,
    goal: String,
    location: SrcSpan,
    obligation: Obligation,
}

/// `None` means the value has a type the encoder does not model; it is never
/// constrained and cannot appear in a goal.
type Term = Option<String>;

struct Encoder<'a> {
    module: &'a EcoString,
    contracts: &'a HashMap<EcoString, Contract<'a>>,
    declarations: Vec<String>,
    facts: Vec<String>,
    obligations: Vec<Query>,
    inputs: Vec<(EcoString, String)>,
    env: HashMap<EcoString, String>,
    path: Vec<String>,
    counter: usize,
}

impl<'a> Encoder<'a> {
    fn new(module: &'a EcoString, contracts: &'a HashMap<EcoString, Contract<'a>>) -> Self {
        Self {
            module,
            contracts,
            declarations: vec![],
            facts: vec![],
            obligations: vec![],
            inputs: vec![],
            env: HashMap::new(),
            path: vec![],
            counter: 0,
        }
    }

    fn function(&mut self, function: &TypedFunction) {
        for argument in &function.arguments {
            if let Some(name) = argument.names.get_variable_name()
                && let Some(symbol) = self.fresh(name, &argument.type_)
            {
                self.inputs.push((name.clone(), symbol.clone()));
                let _ = self.env.insert(name.clone(), symbol);
            }
        }
        let _ = self.statements(&function.body);
    }

    fn fresh(&mut self, hint: &str, type_: &Arc<Type>) -> Option<String> {
        let sort = sort_of(type_)?;
        self.counter += 1;
        let symbol = format!("{}_{}", sanitise(hint), self.counter);
        self.declarations
            .push(format!("(declare-const {symbol} {sort})"));
        Some(symbol)
    }

    fn assume(&mut self, term: &str) {
        let fact = guarded(&self.path, term);
        self.facts.push(format!("(assert {fact})"));
    }

    fn prove(&mut self, goal: String, location: SrcSpan, obligation: Obligation) {
        self.obligations.push(Query {
            facts: self.facts.len(),
            path: self.path.clone(),
            goal,
            location,
            obligation,
        });
    }

    fn statements(&mut self, statements: &[TypedStatement]) -> Term {
        let saved = self.env.clone();
        let mut last = None;
        for statement in statements {
            last = self.statement(statement);
        }
        self.env = saved;
        last
    }

    fn statement(&mut self, statement: &TypedStatement) -> Term {
        match statement {
            Statement::Expression(expression) => self.expression(expression),

            Statement::Assignment(assignment) => {
                let value = self.expression(&assignment.value);
                match (&assignment.kind, &assignment.pattern) {
                    (
                        AssignmentKind::Let | AssignmentKind::Generated,
                        Pattern::Variable { name, type_, .. },
                    ) => self.bind(name, type_, value),
                    // Other patterns bind nothing the encoder models; the
                    // names they introduce are simply unknown.
                    _ => {}
                }
                None
            }

            Statement::Assert(assert) => {
                let value = self.expression(&assert.value);
                match (assert.kind, value) {
                    (AssertKind::Requires, Some(term)) => self.assume(&term),
                    (AssertKind::Ensures, Some(term)) => {
                        self.prove(term.clone(), assert.location, Obligation::Ensures);
                        self.assume(&term);
                    }
                    (AssertKind::Ensures, None) => {
                        self.prove("false".into(), assert.location, Obligation::Ensures)
                    }
                    // Plain asserts are runtime checks the user wrote; v0 only
                    // learns from them.
                    (AssertKind::Plain, Some(term)) => self.assume(&term),
                    (_, None) => {}
                }
                None
            }

            Statement::Use(_) => None,
        }
    }

    fn bind(&mut self, name: &EcoString, type_: &Arc<Type>, value: Term) {
        let Some(symbol) = self.fresh(name, type_) else {
            return;
        };
        if let Some(value) = value {
            self.facts.push(format!("(assert (= {symbol} {value}))"));
        }
        let _ = self.env.insert(name.clone(), symbol);
    }

    fn unknown(&mut self, type_: &Arc<Type>) -> Term {
        self.fresh("unknown", type_)
    }

    fn expression(&mut self, expression: &TypedExpr) -> Term {
        match expression {
            TypedExpr::Int { int_value, .. } => Some(int_literal(&int_value.to_string())),

            TypedExpr::Var {
                name, constructor, ..
            } => match &constructor.variant {
                ValueConstructorVariant::LocalVariable { .. } => self.env.get(name).cloned(),
                ValueConstructorVariant::Record { name, .. } if constructor.type_.is_bool() => {
                    Some(if name == "True" { "true" } else { "false" }.into())
                }
                _ => self.unknown(&constructor.type_),
            },

            TypedExpr::NegateBool { value, .. } => {
                Some(format!("(not {})", self.expression(value)?))
            }
            TypedExpr::NegateInt { value, .. } => Some(format!("(- {})", self.expression(value)?)),

            TypedExpr::BinOp {
                operator,
                left,
                right,
                type_,
                ..
            } => self.binop(*operator, left, right, type_),

            TypedExpr::Block { statements, .. } => self.statements(statements),

            TypedExpr::Pipeline {
                first_value,
                assignments,
                finally,
                ..
            } => {
                let saved = self.env.clone();
                for step in std::iter::once(first_value).chain(assignments.iter().map(|(a, _)| a)) {
                    let value = self.expression(&step.value);
                    self.bind(&step.name, &step.value.type_(), value);
                }
                let result = self.expression(finally);
                self.env = saved;
                result
            }

            TypedExpr::Case {
                subjects,
                clauses,
                type_,
                ..
            } => self.case(subjects, clauses, type_),

            TypedExpr::Call {
                fun,
                arguments,
                type_,
                location,
                ..
            } => self.call(fun, arguments, type_, *location),

            other => {
                // Still walk nothing inside: obligations hidden in unsupported
                // expressions are left to the runtime asserts.
                self.unknown(&other.type_())
            }
        }
    }

    fn binop(
        &mut self,
        operator: BinOp,
        left: &TypedExpr,
        right: &TypedExpr,
        type_: &Arc<Type>,
    ) -> Term {
        let l = self.expression(left);
        // The right operand of && / || only runs when the left allows it, so
        // obligations inside it hold under that condition.
        let r = match (operator, &l) {
            (BinOp::And, Some(l)) => self.under(l.clone(), |this| this.expression(right)),
            (BinOp::Or, Some(l)) => self.under(format!("(not {l})"), |this| this.expression(right)),
            _ => self.expression(right),
        };
        let (Some(l), Some(r)) = (l, r) else {
            return self.unknown(type_);
        };
        let op = match operator {
            BinOp::And => "and",
            BinOp::Or => "or",
            BinOp::Eq if sort_of(&left.type_()).is_some() => "=",
            BinOp::NotEq if sort_of(&left.type_()).is_some() => {
                return Some(format!("(not (= {l} {r}))"));
            }
            BinOp::LtInt => "<",
            BinOp::LtEqInt => "<=",
            BinOp::GtInt => ">",
            BinOp::GtEqInt => ">=",
            BinOp::AddInt => "+",
            BinOp::SubInt => "-",
            BinOp::MultInt => "*",
            BinOp::DivInt => "gdiv",
            BinOp::RemainderInt => "grem",
            _ => return self.unknown(type_),
        };
        Some(format!("({op} {l} {r})"))
    }

    fn under<T>(&mut self, condition: String, f: impl FnOnce(&mut Self) -> T) -> T {
        self.path.push(condition);
        let result = f(self);
        let _ = self.path.pop();
        result
    }

    fn case(
        &mut self,
        subjects: &[TypedExpr],
        clauses: &[crate::ast::TypedClause],
        type_: &Arc<Type>,
    ) -> Term {
        let subjects: Vec<Term> = subjects.iter().map(|s| self.expression(s)).collect();
        let result = self.fresh("case", type_);
        let mut previous: Vec<String> = vec![];

        for clause in clauses {
            if clause.guard.is_some() || !clause.alternative_patterns.is_empty() {
                // Unsupported clause shapes: give up on the result, but later
                // clauses must not assume this one failed to match.
                return self.unknown(type_);
            }
            let saved = self.env.clone();
            let mut conditions = vec![];
            for (pattern, subject) in clause.pattern.iter().zip(&subjects) {
                match self.pattern(pattern, subject.as_deref()) {
                    Some(condition) => conditions.extend(condition),
                    None => {
                        self.env = saved;
                        return self.unknown(type_);
                    }
                }
            }
            let matches = conjunction(&conditions);
            let reached = conjunction(
                &previous
                    .iter()
                    .map(|p| format!("(not {p})"))
                    .chain(std::iter::once(matches.clone()))
                    .collect::<Vec<_>>(),
            );
            let then = self.under(reached.clone(), |this| this.expression(&clause.then));
            if let (Some(result), Some(then)) = (&result, then) {
                let fact = guarded(&self.path, &format!("(=> {reached} (= {result} {then}))"));
                self.facts.push(format!("(assert {fact})"));
            }
            self.env = saved;
            previous.push(matches);
        }
        result
    }

    /// The condition under which `pattern` matches, binding variables it
    /// introduces. `None` means the pattern shape is not modelled.
    fn pattern(
        &mut self,
        pattern: &Pattern<Arc<Type>>,
        subject: Option<&str>,
    ) -> Option<Option<String>> {
        match pattern {
            Pattern::Discard { .. } => Some(None),
            Pattern::Variable { name, type_, .. } => {
                self.bind(name, type_, subject.map(String::from));
                Some(None)
            }
            Pattern::Int { int_value, .. } => {
                let subject = subject?;
                Some(Some(format!(
                    "(= {subject} {})",
                    int_literal(&int_value.to_string())
                )))
            }
            Pattern::Constructor {
                name,
                type_,
                arguments,
                ..
            } if type_.is_bool() && arguments.is_empty() => {
                let subject = subject?;
                Some(Some(if name == "True" {
                    subject.to_string()
                } else {
                    format!("(not {subject})")
                }))
            }
            _ => None,
        }
    }

    fn call(
        &mut self,
        fun: &TypedExpr,
        arguments: &[crate::ast::CallArg<TypedExpr>],
        type_: &Arc<Type>,
        location: SrcSpan,
    ) -> Term {
        let values: Vec<Term> = arguments
            .iter()
            .map(|a| self.expression(&a.value))
            .collect();
        let result = self.fresh("call", type_);

        let callee = match fun {
            TypedExpr::Var { constructor, .. } => match &constructor.variant {
                ValueConstructorVariant::ModuleFn { name, module, .. } if module == self.module => {
                    Some(name.clone())
                }
                _ => None,
            },
            _ => None,
        };
        let Some(callee) = callee else {
            return result;
        };
        let Some(contract) = self.contracts.get(&callee) else {
            return result;
        };

        // Contract clauses are encoded in a scope holding only the callee's
        // parameters (and `result`), bound to this call's arguments.
        let saved = std::mem::take(&mut self.env);
        for (parameter, value) in contract.parameters.iter().zip(&values) {
            if let (Some(parameter), Some(value)) = (parameter, value) {
                let _ = self.env.insert(parameter.clone(), value.clone());
            }
        }
        let requires: Vec<Term> = contract
            .requires
            .iter()
            .map(|c| self.expression(c))
            .collect();
        if let Some(result) = &result {
            let _ = self.env.insert(CONTRACT_RESULT.into(), result.clone());
        }
        let ensures: Vec<Term> = contract
            .ensures
            .iter()
            .map(|c| self.expression(c))
            .collect();
        self.env = saved;

        for requirement in requires {
            let goal = requirement.unwrap_or_else(|| "false".into());
            self.prove(
                goal,
                location,
                Obligation::CallRequires {
                    callee: callee.clone(),
                },
            );
        }
        for guarantee in ensures.into_iter().flatten() {
            self.assume(&guarantee);
        }
        result
    }
}

fn sort_of(type_: &Arc<Type>) -> Option<&'static str> {
    if type_.is_int() {
        Some("Int")
    } else if type_.is_bool() {
        Some("Bool")
    } else {
        None
    }
}

fn sanitise(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

fn int_literal(value: &str) -> String {
    let digits: String = value.chars().filter(|c| *c != '_').collect();
    match digits.strip_prefix('-') {
        Some(positive) => format!("(- {positive})"),
        None => digits,
    }
}

fn conjunction(terms: &[String]) -> String {
    match terms {
        [] => "true".into(),
        [one] => one.clone(),
        many => format!("(and {})", many.join(" ")),
    }
}

fn guarded(path: &[String], term: &str) -> String {
    if path.is_empty() {
        term.to_string()
    } else {
        format!("(=> {} {term})", conjunction(path))
    }
}

fn solve(
    declarations: &[String],
    facts: &[String],
    query: &Query,
    inputs: &[(EcoString, String)],
) -> Result<Outcome, Error> {
    let mut script = String::from(PREAMBLE);
    for line in declarations.iter().chain(&facts[..query.facts]) {
        script.push_str(line);
        script.push('\n');
    }
    script.push_str(&format!("(assert {})\n", conjunction(&query.path)));
    script.push_str(&format!("(assert (not {}))\n(check-sat)\n", query.goal));
    if !inputs.is_empty() {
        let symbols: Vec<&str> = inputs.iter().map(|(_, s)| s.as_str()).collect();
        script.push_str(&format!("(get-value ({}))\n", symbols.join(" ")));
    }

    if let Ok(dump) = std::env::var("GLEAM_VERIFY_DUMP") {
        let _ = std::fs::write(format!("{dump}/{}.smt2", query.location.start), &script);
    }

    let solver = std::env::var("GLEAM_Z3").unwrap_or_else(|_| "z3".into());
    let mut child = Command::new(&solver)
        .args(["-in", &format!("-t:{TIMEOUT_MS}")])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| Error::SolverUnavailable(eco_format!("{solver}: {error}").into()))?;
    child
        .stdin
        .take()
        .expect("piped stdin")
        .write_all(script.as_bytes())
        .map_err(|error| Error::SolverUnavailable(error.to_string()))?;
    let output = child
        .wait_with_output()
        .map_err(|error| Error::SolverUnavailable(error.to_string()))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut lines = stdout.lines();

    Ok(match lines.next().map(str::trim) {
        Some("unsat") => Outcome::Proved,
        Some("sat") => {
            let model = lines.collect::<Vec<_>>().join(" ");
            Outcome::Counterexample(
                inputs
                    .iter()
                    .filter_map(|(name, symbol)| Some((name.clone(), model_value(&model, symbol)?)))
                    .collect(),
            )
        }
        Some(other) => Outcome::Unknown(other.to_string()),
        None => Outcome::Unknown(String::from_utf8_lossy(&output.stderr).trim().to_string()),
    })
}

/// Pulls `value` out of z3's `((symbol value) ...)` get-value response.
fn model_value(model: &str, symbol: &str) -> Option<String> {
    let start = model.find(&format!("({symbol} "))? + symbol.len() + 2;
    let rest = &model[start..];
    let mut depth = 0i32;
    for (i, c) in rest.char_indices() {
        match c {
            '(' => depth += 1,
            ')' if depth == 0 => {
                let raw = rest[..i].trim();
                return Some(raw.replace("(- ", "-").replace(')', "").trim().to_string());
            }
            ')' => depth -= 1,
            _ => {}
        }
    }
    None
}
