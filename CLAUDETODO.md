# Plans

## Add clicking on log items support

1. ✓ simply use log::debug! to print the clicked raw position, when clicked on the LOGS block.
2. TODO: log out the raw clicked log item number from top down starting from 0 to x, only account for the mouse click event's row id, and the starting height of the LOGS block, you don't need to calculate for the internal scrolling state of the LOGS itself, just the raw position.
3. combined with the raw log item number and the internal scrolling state of the LOGS block, you can get the exact log item number that the user clicked on.
4.

# Problems

## Scrolling Problem

We are using crossterm raw mode right now on macOS platform, and some of our users are using trackpads. Trackpads has inertia scrolling, which means the system fires more scrolling events even the user's finger leaves the trackpad.
Now we are having a problem that when the user scrolls, then move the mouse, the scroll events are continuously fired, causing more scrolling on the blocks, this is undesired.
