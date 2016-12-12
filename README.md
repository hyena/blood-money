A World of Warcraft webapp that helps players determine the best value for their bloods of sargeras on their realm.

Status
------
Basic downloading of all realm auctions is implemented. It's
slow, sadly, but it's a lot of data.

Todo
----
  - Read token from config (or commandline?)
  - Process auction data.
  - Move all network logic into a background thread.
  - Move these println's into a real logging system.
Fix up hyper and iron dependencies with real versions.

Move the token into a config.toml file.

Currently does not really respect changes in realm lists. Requires a restart to handle those changes.

License
-------

Although I can't imagine someone else using this: MIT, of course.
