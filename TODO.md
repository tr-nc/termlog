# Plans

# Problems

## Scrolling Problem

We are using crossterm raw mode right now on macOS platform, and some of our users are using trackpads. Trackpads has inertia scrolling, which means the system fires more scrolling events even the user's finger leaves the trackpad.
Now we are having a problem that when the user scrolls, then move the mouse, the scroll events are continuously fired, causing more scrolling on the blocks, this is undesired.
