// Reference solution derived from Swarm 0.8.0.0's Farming tutorial.

def tR = turn right end;
def tL = turn left end;
def tB = turn back end;
def forever = \c. c ; forever c end;
def ifC : Cmd Bool -> {Cmd a} -> {Cmd a} -> Cmd a = \test. \then. \else.
  b <- test; if b then else
end;
def while : Cmd Bool -> {Cmd a} -> Cmd Unit = \test. \body.
  ifC test {force body ; while test body} {}
end;
def giveall : Actor -> Text -> Cmd Unit = \r. \thing.
  while (has thing) {give r thing}
end;
def x4 = \c. c; c; c; c end;
def m4 = x4 move end;
def x12 = \c. x4 (c;c;c) end;
def m12 = x12 move end;
def next_row = tB; m12; tL; move; tL end;
def plant_field : Text -> Cmd Unit = \thing.
  x4 (
    x12 (move; place thing; harvest);
    next_row
  )
end;
def harvest_field : Text -> Cmd Unit = \thing.
  x4 (
    x12 (move; ifC (ishere thing) {harvest; pure ()} {});
    next_row
  );
  tL; m4; tR
end;
def harvest_lambdas =
  forever (
    tB; move; tR; harvest_field "lambda"; tR; move; giveall base "lambda"
  )
end;

build {
  stock 1 "lambda";
  tB; move; tR; plant_field "lambda";
};
build {
  harvest_lambdas
};

make "log"; make "log"; make "board"; make "board"; make "boat";
build {
  require "boat";
  move; move; move; turn right; move; move; move; grab;
  turn back; move; move; move; turn left; move; move; move;
  give base "water";
};
wait 24;
make "curry"
