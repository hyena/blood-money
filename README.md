A World of Warcraft webapp that helps players determine the best value for their bloods of sargeras on their realm.

Quickstart
----------
  1. Compile blood-money
  2. Make an account on https://dev.battle.net/ and generate an
     API key
  3. Run `blood-money <api key>`
  4. Look at http://localhost:3000

Todo
----
  - eu support is presently a hack and a separate branch. Fix this.
  - Read token from config (or stick with commandline?)
  - Implement refresh in main thread
  - Move these println's into a real logging system.
  - Fix up hyper and iron dependencies with real versions.
  - Remove earthen-ring-grabber.rs when the main
    implementation is complete.
  - Save data between runs and use it when bringing the service
    back up.
  - I switched the deserialization library from `serde` back to
    `rustc_serialize` because `serde` was dying on Blizzard's
    unicode in auction owner names. `utf8_lossy()` conversion
    doesn't seem to strip enough. Until I fix this, stick with
    `rustc_serialize` since it seems more permissive.
  - The threading model is presently fairly serial and could be
    improved such that it was hurt less by stragglers or one
    buggy realm.

Things that we might get to if this became more serious:
  - Currently does not respect changes in realm lists.
    Requires a restart to handle those changes.

License
-------
Although I can't imagine someone else using this: MIT, of course.
