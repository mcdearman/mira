# Signals

A signal is a value derived from the world and from other signals, which the engine keeps
true. It is how to write a rule once, as what it is, instead of as handlers that set and
clear a shared flag and one day disagree.

The example this was built around is a bug in Age of Empires 4. A team holding every sacred
site wins when its clock runs out, and the clock stops while an enemy stands on a site. Step
onto one site: the clock stops. Step onto a second, lose the units on the first: the clock
starts again, with an enemy still standing on a site. Somewhere a flag was cleared by the
units leaving one site that should have stayed set for the other.

The rule, said plainly: *the clock runs while red holds every site and no blue unit stands on
any site red holds.* As signals:

```rust
app.add_signal("red.holds_all", |sites: Query<&Site>| {
    sites.iter().all(|site| site.holder == Some(Team::Red))
})
.add_signal("blue.contesting", |units: Query<(&Team, &Transform)>, sites: Query<(&Site, &Transform)>| {
    units.iter().any(|(team, at)| {
        *team == Team::Blue && sites.iter().any(|(site, centre)| {
            site.holder == Some(Team::Red) && at.translation.distance(centre.translation) < 2.0
        })
    })
});

let signals = app.world.resource_mut::<Signals>();
signals.define("blue.calm", Op::Not, ["blue.contesting"]);
signals.define("red.clock.running", Op::And, ["red.holds_all", "blue.calm"]);
signals.define("red.lost_a_site", Op::Not, ["red.holds_all"]);
signals.define("red.clock", Op::Timer, ["red.clock.running", "red.lost_a_site"]);
signals.set("win_after", 600.0);
signals.define("red.wins", Op::Compare(Compare::GreaterOrEqual), ["red.clock", "win_after"]);
```

No code sets the clock or a paused flag, so there is nothing to get out of step: units can
walk on and off and die in any order. That game is a test in `src/signal.rs`.

## The graph

Signals are nodes with names, in the `Signals` resource.

- A **source** reads the world: `app.add_signal(name, f)`, where `f` is a function with
  system parameters returning `bool` (the same shape as a
  [run condition](SCHEDULING.md#conditions)), or `signals.measure(name, f)` for a number.
- Every other node is data: an operation and the names of its inputs,
  `signals.define(name, op, inputs)`.

| `Op` | Value |
| --- | --- |
| `Constant(v)` | `v`; `signals.set(name, v)` is short for it |
| `And`, `Or`, `Not` | as they say |
| `Count` | how many inputs are true |
| `Sum` | the inputs added up |
| `Compare(how)` | the first input against the second |
| `Select` | the second input if the first is true, else the third |
| `HeldFor(seconds)` | true once its input has been true that long without a break |
| `Timer` | seconds its first input has been true; a true second input resets it |

A value is a truth or a number; where one is wanted and the other given, true is 1 and any
number but 0 is true. Inputs may be defined after the nodes that read them. Two nodes that
read each other are allowed: one of them reads the other a frame late, and says so.

Signals are worked out once a frame in `Stage::PreUpdate`, in the set `"signals"`, from the
world as it is then.

## Using them

```rust
fn hud(signals: Res<Signals>) {
    let left = 600.0 - signals.number("red.clock");
    let stopped = !signals.is_true("red.clock.running");
}

app.add_systems(Stage::Update, (
    announce_victory.run_if(signal_became_true("red.wins")),
    play_alarm.run_if(signal("blue.contesting")),
));
```

`signals.changed(name)`, `became_true` and `became_false` are about the last update, and each
change is also sent as a `SignalChanged` event.

## Watching and changing them live

Because every node but a source is data, the graph can be read and rewired while the game
runs, which is what a signal graph viewer will do:

```rust
for node in signals.graph() {
    // name, kind ("source", "and", "timer", …), inputs, value, changed, forced, problem
}
signals.set("win_after", 30.0);                        // change a setting
signals.connect("red.clock.running", 1, "always");     // connect an input elsewhere
signals.force("blue.contesting", Some(Signal::Bool(false))); // hold a node's output
signals.force("blue.contesting", None);                // let it go
signals.define("red.clock.running", Op::Or, ["red.holds_all", "cheat"]); // replace a rule
signals.set_op("blue.calm", Op::Count);                // the same inputs, another rule
signals.set_inputs("red.wins", ["red.clock"]);          // the same rule, other inputs
```

Redefining a node keeps its value and what it has timed, so changing a rule doesn't restart
its clocks. An input connected to a name that doesn't exist reads false and is reported in
the node's `problem`. `signals.to_value()` is the whole graph as plain data.

## The viewer

With the game listening for debuggers ([LIVE.md](LIVE.md#the-debug-connection)), this draws
the graph in a terminal and keeps it up to date as you play. The sacred-site game is an
example to try it on: two sites seen from above through an orthographic camera, scouts
wandering on and off them, and a bar that fills as red's clock runs, all drawn from the
signals (`--headless` runs the same game with no window):

```sh
cargo run --bin mira-play -- examples/sacred_sites   # the game
cargo run --bin mira-debug -- watch     # in another terminal
```

```text
frame 4821  80.4s  running  failures: 0

○ red.wins = false  (compare GreaterOrEqual)
├─ ● red.clock = 7  (timer)
│  ├─ ○ red.clock.running = false  (and) *
│  │  ├─ ● red.holds_all = true  (source)
│  │  └─ ○ blue.calm = false  (not) *
│  │     └─ ● blue.contesting = true  (source) *
│  └─ ○ red.lost_a_site = false  (not)
│     └─ ● red.holds_all = true  (source)
└─ ● win_after = 600  (constant)
```

Each signal nothing else reads is at the left, with what it reads below it. `●` is true (or
not zero), `*` changed on the last update, `!` is forced. Change the graph from another
terminal and watch it follow:

```sh
cargo run --bin mira-debug -- signal_set name=win_after value=30
cargo run --bin mira-debug -- signal_force name=blue.contesting value=false
cargo run --bin mira-debug -- signal_connect name=red.clock.running input=1 to=always
cargo run --bin mira-debug -- signal_define name=blue.calm op=held_for seconds=3 inputs='["blue.away"]'
cargo run --bin mira-debug -- signal_rename name=blue.calm to=blue.quiet
```

## From a plugin

A plugin gives the graph its own facts by setting signals from a system, builds rules on them
as data, and reads any signal by name. In Meadow:

```
let u1 = M.setSignal sys "blue.contesting" contesting in      -- a fact, from the plugin's own queries
let u2 = M.defineSignal sys "red.clock.running" M.signalAnd ["red.holds_all", "blue.calm"] in
let u3 = M.defineSignal sys "red.clock" M.signalTimer ["red.clock.running", "red.clock.reset"] in
let u4 = M.defineSignal sys "red.wins" M.signalGreaterOrEqual ["red.clock", "win_after"] in
if M.signalIsTrue sys "red.wins" then announce sys else ()
```

Each rule is one node: `M.defineSignal` takes how it is worked out (`M.signalAnd`,
`M.signalOr`, `M.signalNot`, `M.signalTimer`, the comparisons, …) and its inputs by name;
`M.defineHeldFor` is the one with a number of seconds.

In Haskell:

```haskell
addSystem_ app "rules" Update $ \sys -> do
  setSignal sys "blue.contesting" =<< anyBlueOnARedSite sys   -- a fact, from the plugin's own queries
  defineRule sys "red.clock" $
    timerReset (sig "red.holds_all" .&&. notS (sig "blue.contesting"))
               (notS (sig "red.holds_all"))
  defineRule sys "red.wins" (sig "red.clock" .>=. number 600)
  won <- signalIsTrue sys "red.wins"
  when won (announce sys)
```

In Haskell a rule can also be an expression: `sig` names a signal, `.&&.`, `.||.`, `notS` and the
comparisons (`.<.`, `.>=.`, …) combine them, `+` adds, and `timer`, `timerReset`, `heldFor`,
`countS` and `selectS` are the rest. `defineRule` turns the expression into nodes, naming its
parts after the rule (`red.clock#1`, …), so the whole of it shows in the graph and can be
rewired there. `defineSignal` defines one node at a time, as the other languages do.

What a plugin sets with `setSignal` is a fact: a source, as a function the game gives the
graph is, told what it is each time the plugin works it out. It is not among the rules that
are kept or written as text. A setting, which is the game's to begin with and anyone's to
change after, is defined as a rule with no inputs: `M.defineNumber sys "win_after" 60.0` in
Meadow (`M.defineTruth` for a truth), `defineSignal sys "win_after" (SignalNumber 60) []` in
Haskell (`SignalTruth`), `MIRA_SIGNAL_NUMBER` and `MIRA_SIGNAL_TRUTH` in C. The sacred
sites are written this way: [`examples/sacred_sites/Sites.mw`](../examples/sacred_sites/Sites.mw),
and in Haskell [`examples/haskell/sacred_sites/Sites.hs`](../examples/haskell/sacred_sites/Sites.hs).

In C these are `signal_set`, `signal_get` and `signal_define` in `include/mira.h`; in Rust,
`System::set_signal`, `signal`, `define_signal`. What a plugin sets in one frame the graph has
in the next. Defining the same rule again changes nothing, so a plugin can define its rules
every frame or once; either way they are there again after a reload, with their clocks intact.

## Keeping rules

Rules worked out while the game ran can be kept. `signals.save_rules("rules.json")` writes
every signal that isn't a source (its operation, inputs, and constants' values) and
`signals.load_rules("rules.json")` defines them again, replacing rules of the same name;
sources come from the game's code and are only named, as inputs. Where boxes were put in the
in-game panel is kept in the same file, under `layout`. A file that can't be read
changes nothing and says which rule was wrong. From outside: `mira-debug signals_save
path=rules.json` and `signals_load`.

```json
{"version": 1, "signals": [
  {"name": "win_after", "op": "constant", "value": 60.0},
  {"name": "blue.calm", "op": "not", "inputs": ["blue.contesting"]},
  {"name": "red.clock", "op": "timer", "inputs": ["red.clock.running", "red.lost_a_site"]}
]}
```

## Rules as text

The rules can also be kept as text, a line each, and the text and the graph are then one
thing seen two ways:

```text
-- Red wins by holding every site for a minute without a break.
win_after = 60
blue.calm = not blue.contesting
red.clock = timer(red.holds_all and blue.calm, not red.holds_all)
red.wins = red.clock >= win_after
```

```rust
app.signal_rules("sites.rules");
```

From then on the file and the running graph are kept the same. Save the file and the game
follows at once, keeping what its clocks have timed. Rewire a node in the panel, or change
something over the debug connection, and the file follows: only the lines whose rules
changed are written again, and comments, blank lines and every other line stay exactly as
they were typed. If the file is not there it is written from the rules the game's code
defines. Try it with `cargo run --bin mira-play -- examples/sacred_sites --rules sites.rules` and the file
open in an editor beside it.

A file saved half typed changes nothing: the graph stays as it was, the mistake is logged
with its line and column (and is in `RulesFile::problem`), and nothing is written over the
file until it can be read again.

| Written | Is |
|---|---|
| `a and b and c`, `a or b`, `not a` | true when all are, when any is, when it is not |
| `a < b`, `<=`, `==`, `>=`, `>` | a comparison |
| `a + b + 3` | the sum |
| `if a then b else c` | `b` while `a` is true, otherwise `c` |
| `timer(a)`, `timer(a, reset)` | the seconds `a` has been true; a true `reset` sets it back to nought |
| `held(a, 3)` | true once `a` has been true for 3 seconds without a break |
| `count(a, b, c)` | how many are true |
| `600`, `true` | a number, a truth |
| `-- …` | a comment, to the end of the line |

A name is letters, digits, `_`, `.` and `#`; any other is written between backticks. A rule
is one line. An expression of more than one operation is several nodes in the graph, the
inner ones named after the rule (`red.clock#1`, …); they are written inside their rule's
line, and a node named that way that something else comes to read gets a line of its own.
Sources are the game's code and have no line: they are only named.

In code, `Rules::read(text)` reads a text, `rules.apply(&mut signals)` makes the graph what it
says, `rules.follow(&signals)` makes it say what the graph is, and `Expr::read` and
`to_text` do one expression.

Keep a rule in one place. A plugin that defines a rule every frame will put it back as the
plugin has it, whatever the file or the panel made of it.

## What isn't here yet

- The text beside the graph in the panel itself: for now it is a file, open in any editor.
- Where the panel's boxes sit, kept with the text (it is kept with the JSON rules).
- Rules over more than one line.
- Sources written in a plugin as functions the engine calls (a plugin sets its facts from a
  system instead).
- Signals that carry more than a truth or a number (an entity, a set of entities).
- Sources are asked every frame; they are not yet skipped when what they read hasn't changed.
