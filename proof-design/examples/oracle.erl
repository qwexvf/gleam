-module(oracle).
-export([main/0]).

%% Runs every contracted function on all inputs in a range that satisfy its
%% requires clauses, and reports which inputs hit a runtime assert failure.
%% This exercises the generated Erlang, not the SMT encoding.
check(Name, Arity, Pre) ->
    R = lists:seq(-40, 40),
    Inputs = case Arity of
        1 -> [[A] || A <- R];
        2 -> [[A, B] || A <- R, B <- R]
    end,
    Ok = [I || I <- Inputs, Pre(I)],
    Fails = [I || I <- Ok, failed(Name, I)],
    io:format("~-18s inputs=~5w failures=~5w first=~p~n",
              [Name, length(Ok), length(Fails), case Fails of [] -> none; [F|_] -> F end]).

failed(Name, Args) ->
    try apply(ctest, Name, Args), false
    catch error:#{gleam_error := assert} -> true
    end.

main() ->
    check(withdraw, 2, fun([B, A]) -> B >= 0 andalso A > 0 andalso A =< B end),
    check(withdraw_with_fee, 2, fun([B, A]) -> B >= 0 andalso A > 0 andalso A =< B end),
    check(safe_withdraw, 2, fun([B, _]) -> B >= 0 end),
    check(careless, 1, fun(_) -> true end),
    check(half, 1, fun([N]) -> N < 0 end),
    check(half_exact, 1, fun([N]) -> N < 0 end),
    check(sum_to, 1, fun([N]) -> N >= 0 end),
    check(sum_to_strict, 1, fun([N]) -> N >= 0 end),
    check(rem_sign, 2, fun([_, B]) -> B =/= 0 end).
