A World of Warcraft webapp that helps players determine the best value for their bloods of sargeras on their realm.

Status
------
Basic downloading of all realm auctions is implemented. It's
slow, sadly, but it's a lot of data.

Todo
----
  - Read token from config (or stick with commandline?)
  - Implement refresh in main thread
  - Move these println's into a real logging system.
  - Fix up hyper and iron dependencies with real versions.
  - Provide a better message when initial values aren't yet
    available.
  - Remove earthen-ring-grabber.rs when the main
    implementation is complete.

Things that we might get to if this became more serious:
  - Currently does not respect changes in realm lists.
    Requires a restart to handle those changes.

License
-------
Although I can't imagine someone else using this: MIT, of course.
