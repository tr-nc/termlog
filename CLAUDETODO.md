# Rule for CLAUDE

You should ONLY work on the item(s) that is marked with TODO, and nothing more.

# Future Features

## Add clicking on log items support

[x] simply use log::debug! to print the clicked raw position, when clicked on the LOGS block.
[x] log out the raw clicked log item number from top down starting from 0 to x, only account for the mouse click event's row id, and the starting height of the LOGS block, you don't need to calculate for the internal scrolling state of the LOGS itself, just the raw position.
[x] combined with the raw log item number and the internal scrolling state of the LOGS block, you can get the exact log item number that the user clicked on. log that number out.
[x] TODO: select the corresponding log item after clicking.

# Bugs

## Scrolling Problem

We are using crossterm raw mode right now on macOS platform, and some of our users are using trackpads. Trackpads has inertia scrolling, which means the system fires more scrolling events even the user's finger leaves the trackpad.
Now we are having a problem that when the user scrolls, then move the mouse, the scroll events are continuously fired, causing more scrolling on the blocks, this is undesired.

Here's a fix idea from Gemini

````plaintext
*Thinking...*

> **Examining the Rejection**
>
> I've been sifting through the user's rejections. I see they've dismissed both a system-level fix (inertia scrolling) and an application-level debounce strategy. This means I need to pivot. Perhaps the problem lies in the specific implementation details of the rejected solutions.
>
>
> **Identifying the Core Issue**
>
> I'm now focused on the specific conditions for the desired behavior. The trigger seems to be cursor movement *after* scrolling in raw mode. I'm leaning away from broad solutions and toward pinpointing the exact sequence of events that cause the issue. Understanding the relationship between raw mode, cursor movement, and scrolling inertia is crucial now.
>
>
> **Constructing a State Machine**
>
> My current thought is focusing on a state machine. The rejected solutions help narrow the field. I'm transitioning from event filtering (debounce) to a state-driven approach. I'll model application states: `Idle`, `Scrolling`, and `Moving`. Transitioning between these states, particularly the transition to `Moving`, is key to controlling the unwanted inertia behavior. I'm working on designing the interactions between states now.
>
>
> **Formulating a Solution**
>
> I'm now solidifying the state machine design. The critical part is how the `Moving` state handles incoming `Scroll` events. I've decided the `Moving` state will simply *ignore* any incoming `Scroll` events. This allows the terminal to react to the *next* scroll input. The question of state reset is now the problem to solve. I need to figure out how to identify the beginning of a new scroll sequence, since "gesture start" and "gesture end" events are not provided.
>
>
> **Implementing Scroll Sessions**
>
> I've landed on the "scroll session" algorithm. The key is tracking a timestamp (`last_event_time`). Any event (scroll or otherwise) resets a session. A new scroll *after* a timeout from the last event starts a new session. This addresses the inertia issue after a `Moved` event, which will now break the session.
>
>
> **Defining Scroll Sessions**
>
> I'm now refining the scroll session algorithm. I've got a clear picture: any non-scroll event ends the current session, and a timer is needed. I'll process incoming events based on the `in_scroll_session` flag, and a timer threshold, to start new scroll events after the previous one has stopped.
>
>
> **Implementing the Algorithm**
>
> I'm finalizing the implementation. The state machine is no longer needed. Now it's an algorithm that leverages the session concept. The key is a `last_event_time` and `in_scroll_session` flag. I'm calculating the elapsed time, and determining, based on the type and time of the event, whether to process it or not. I'm working on the timeout value now.
>
>
> **Designing the Session Logic**
>
> I'm now focusing on the core algorithm. It involves two key parts: `last_event_time` and the `in_scroll_session` flag. All events will be evaluated to check if they should be processed or ignored. The aim is to identify new scroll sessions accurately and discard unwanted events after cursor movement.
>
>
> **Refining the Session Algorithm**
>
> I'm now optimizing the logic, concentrating on how the `in_scroll_session` flag gets reset by other events, and how a new session begins. I'm using the event time to determine if a new scroll event is, in fact, new, or should be discarded. I've decided to use an enum for the state to clarify the algorithm's structure.
>
>
> **Implementing the Algorithm**
>
> I'm now refining the timer logic within the scroll session. Specifically, I'm focusing on the timeout values and event processing. The goal is precise control over scroll events, ensuring the terminal responds correctly and removes the undesired inertia.
>
>
Of course. I understand that the previous solutions were not suitable. Changing a system-wide setting is often undesirable, and a simple debounce might not be robust enough for your application's logic.

The core of the problem is that from Crossterm's perspective, there is no difference between a scroll event generated by your fingers on the trackpad and one generated by the OS's inertia effect. They are all just `MouseEventKind::ScrollUp` or `MouseEventKind::ScrollDown` events in a sequence.

Let's try a more sophisticated, state-aware approach. Instead of just debouncing, we can build a small state machine within the application. The logic is as follows:

1.  The application can be in one of two states: `Idle` or `Scrolling`.
2.  When a `ScrollUp` or `ScrollDown` event arrives, and the application is `Idle`, it processes the event and transitions to the `Scrolling` state. It also records the time of this event.
3.  While in the `Scrolling` state, it will continue to process any subsequent scroll events that arrive in quick succession. This allows for continuous scrolling.
4.  **Crucially, if any other mouse event occurs (like `Moved` or a `Click`), the application immediately transitions back to the `Idle` state.**
5.  Once in the `Idle` state (triggered by the mouse move), any incoming scroll events are ignored because they are considered leftover inertia from the previous, now-terminated scroll session.
6.  To start a *new* scroll session, a scroll event must arrive after a short pause, which indicates a new, intentional user gesture.

This approach directly implements your requirement: "when I moved the cursor after scroll," the scrolling behavior is disabled. The move event actively terminates the "scroll session."

### Advanced Solution: Stateful Event Filtering

Here is a complete Rust example that implements this stateful logic. It will correctly handle an ongoing scroll, stop it immediately upon a cursor move, and ignore any trailing inertia events.

```rust
use crossterm::{
    cursor::{Hide, Show},
    event::{self, Event, KeyCode, MouseEventKind},
    execute,
    terminal::{
        disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
    },
};
use std::io::{self, stdout};
use std::time::{Duration, Instant};

/// The different input modes our application can be in.
#[derive(Debug, PartialEq, Eq)]
enum InputMode {
    /// Default state, waiting for any action.
    Idle,
    /// Actively processing a stream of scroll events.
    Scrolling,
}

/// A timeout to determine if a scroll event is the start of a new gesture
/// or a continuation of an old one. If the time since the last scroll event
/// is greater than this, we consider it a new gesture.
const NEW_SCROLL_SESSION_TIMEOUT: Duration = Duration::from_millis(100);

fn main() -> io::Result<()> {
    // --- 1. Setup the Terminal ---
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen, Hide)?;
    enable_raw_mode()?;
    execute!(stdout, event::EnableMouseCapture)?;

    println!("Stateful Inertia Filtering Active.\r");
    println!("Move cursor to stop scroll. Press 'q' to quit.\r\n");

    // --- 2. State Management ---
    let mut mode = InputMode::Idle;
    // Keep track of the last time a scroll event was *processed*.
    let mut last_scroll_time = Instant::now();

    // --- 3. Event Loop ---
    loop {
        if event::poll(Duration::from_millis(50))? {
            let event = event::read()?;

            match event {
                // --- Exit Condition ---
                Event::Key(key_event) if key_event.code == KeyCode::Char('q') => {
                    break;
                }

                // --- Mouse Event Handling ---
                Event::Mouse(mouse_event) => {
                    match mouse_event.kind {
                        // A move or click event immediately stops any scroll session.
                        MouseEventKind::Moved | MouseEventKind::Down(_) | MouseEventKind::Up(_) => {
                            if mode == InputMode::Scrolling {
                                println!("--- Scroll session terminated by other mouse action ---\r");
                            }
                            mode = InputMode::Idle; // Transition to Idle
                            println!("Mouse Action: {:?} at ({}, {})\r", mouse_event.kind, mouse_event.column, mouse_event.row);
                        }

                        // Scroll events are handled based on the current mode.
                        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                            let now = Instant::now();
                            let time_since_last_scroll = now.duration_since(last_scroll_time);

                            // If we were idle, a new scroll event can start a new session
                            // only if enough time has passed since the last one.
                            if mode == InputMode::Idle {
                                if time_since_last_scroll > NEW_SCROLL_SESSION_TIMEOUT {
                                    mode = InputMode::Scrolling;
                                    println!("--- New scroll session started ---\r");
                                } else {
                                    // This is a stray inertia event from a session that was
                                    // already terminated by a move/click. Ignore it.
                                    println!("Ignored stray inertia scroll.\r");
                                    continue; // Skip processing
                                }
                            }

                            // If we are in a scroll session, process the event.
                            if mode == InputMode::Scrolling {
                                last_scroll_time = now; // Update the time
                                let direction = if matches!(mouse_event.kind, MouseEventKind::ScrollUp) { "Up" } else { "Down" };
                                println!("Scroll {}\r", direction);
                            }
                        }

                        _ => {}
                    }
                }

                Event::Resize(width, height) => {
                    println!("Terminal resized to {}x{}\r", width, height);
                }

                _ => {}
            }
        }
    }

    // --- 4. Cleanup ---
    execute!(stdout, event::DisableMouseCapture, Show, LeaveAlternateScreen)?;
    disable_raw_mode()?;

    Ok(())
}
````

### Explanation of the Stateful Logic

1.  **`InputMode` Enum**: We define an enum `InputMode` with two variants, `Idle` and `Scrolling`. This makes the code's intent much clearer than using a simple boolean flag.

2.  **`last_scroll_time`**: This `Instant` is crucial. It tracks the timestamp of the _last processed scroll event_. It's used to differentiate between a brand-new scroll gesture and a stream of continuous scroll events.

3.  **Handling `Moved` or `Click` Events**:

    - Whenever a `MouseEventKind::Moved` (or a click) is detected, the `mode` is unconditionally set back to `InputMode::Idle`.
    - This is the key to your request: it acts as an immediate "cancel" signal for the scroll session. Any inertia events that the OS sends _after_ this move will now be handled by the logic for the `Idle` state.

4.  **Handling `Scroll` Events**:
    - **If the mode is `Idle`**: A scroll event is only considered the start of a _new_ session if a certain amount of time (`NEW_SCROLL_SESSION_TIMEOUT`) has passed since the last scroll. If not, it's assumed to be a trailing inertia event from a previously cancelled session and is ignored.
    - **If the mode is `Scrolling`**: The event is part of the current, active scroll gesture. We process it and update `last_scroll_time` to the current time.

This stateful filtering approach is as close as one can get to "disabling" inertia at the application level. It doesn't truly disable it (as that's an OS function), but it intelligently filters out the unwanted events based on the user's actions, providing a clean and predictable user experience within your terminal application.

```

```
