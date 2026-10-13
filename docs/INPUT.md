# Input

## Keys and the mouse, as they are

`ButtonInput<KeyCode>` and `ButtonInput<MouseButton>` say what is held down, what was
pressed this frame and what was let go. `Mouse` has how far it moved, how far the wheel
turned, and where the cursor is.

## Actions

A game does better to ask for what a key is *for*. `Actions` is a resource that knows which
keys and mouse buttons each of the game's actions is bound to:

```rust
fn bind(mut actions: ResMut<Actions>) {
    actions.bind("jump", KeyCode::Space).bind("fire", MouseButton::Left).bind("fire", KeyCode::KeyF);
    actions.bind_axis("across", KeyCode::KeyA, KeyCode::KeyD);
    actions.bind_axis("along", KeyCode::KeyS, KeyCode::KeyW);
}

fn play(actions: Res<Actions>) {
    if actions.just_pressed("jump") { /* … */ }
    let way = actions.direction("across", "along");   // no longer than one
}
```

| | |
|---|---|
| `pressed`, `just_pressed`, `just_released` | of an action: any of what it is bound to presses it |
| `value` | of an axis, from -1 to 1: the first of its pair pulls it down, the second up, both or neither leave it at nought |
| `direction` | two axes as one direction, so that going both ways at once is no faster |

They are brought up to date at the start of each frame, from whatever pressed the keys: a
person, a test, or an agent playing input in from outside.

## Fingers

`Touches` has the fingers that are down on a screen (`iter()`: each with an `id`, where it
went down and where it is), and what they did this frame that a game would name:

```rust
touches.swiped();    // Some(Swipe::Up), the frame a finger has gone far enough that way
touches.tapped();    // Some(where), the frame a finger is lifted from where it went down
touches.gestures();  // all of them, where there was more than one finger
```

A swipe is told as soon as the finger is `swipe_distance` (36 pixels, to begin with) from
where it went down, not when it is lifted, so the game answers at once. A finger swipes
once, and one that has swiped is no tap when lifted.

Actions are bound to these as they are to keys, and the game does not know which it was:

```rust
actions.bind("up", KeyCode::KeyW).bind("up", Swipe::Up).bind("up", Binding::Tap);
```

A swipe or a tap is over in the frame it happens, so it presses its action for that frame:
`just_pressed` is the question to ask. In a bindings file they are `SwipeUp`, `SwipeDown`,
`SwipeLeft`, `SwipeRight` and `Tap`.

`touches.mouse_too = true` has the left mouse button stand for a finger, to try at a desk
a game made for a phone. A test or an agent draws a finger across the screen with
`InjectedInput::swipe(from, to, frames)`, or from outside the game with
`mira-debug input touch=[300,500] touch_to=[300,380] frames=3` (leave `touch_to` out for a tap).

## Letting the player choose

```rust
actions.rebind("jump");          // the next key or mouse button pressed is jump's from now on
actions.rebinding();             // Some("jump") until one is
actions.cancel_rebind();
```

While a key is being chosen no action is pressed, so the key chosen does not also play the
game. `actions.bindings("jump")` gives what it is bound to, each with a `name()` to show.

## Keeping what was chosen

```rust
actions.save("bindings.txt")?;
actions.load("bindings.txt")?;   // after the game's own binds; no file is no error
```

The file is a line an action, as it would be written by hand:

```
fire = MouseLeft, KeyF
jump = Space
across = KeyA/KeyD, ArrowLeft/ArrowRight
```

Keys are named as winit names them (`Space`, `KeyW`, `ArrowLeft`, `Digit1`, `ShiftLeft`,
`F5`); mouse buttons are `MouseLeft`, `MouseRight`, `MouseMiddle`. A file with a mistake in
it is told of, with the line, and changes nothing.

## What there is not yet

Gamepads, and sticks and triggers as axes. Two fingers together (a pinch, a turn), a
finger held as a stick, and buttons drawn on the screen. Choosing a swipe or a tap with
ebind: it listens for keys and mouse buttons only.