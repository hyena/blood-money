A World of Warcraft webapp that helps players determine the best value for their bloods of sargeras on their realm.

Quickstart
----------
  1. Compile blood-money
  2. Make an account on https://dev.battle.net/ and generate an
     API key
  3. Run `blood-money <api key> (us|eu)`
  4. Look at http://localhost:3000/blood-money or http://localhost:3001/blood-money-eu depending on
     how blood-money was launched.

Todo
----
  - Read token from config (or stick with commandline?)
  - Move these println's into a real logging system.
  - Remove earthen-ring-grabber.rs when the main
    implementation is complete.
  - Save data between runs and use it when bringing the service
    back up.
  - The threading model is presently fairly serial and could be
    improved such that it was hurt less by stragglers or one
    buggy realm.

Things that we might get to if this became more serious:
  - Currently does not respect changes in realm lists.
    Requires a restart to handle those changes.

License
-------
Although I can't imagine someone else using this: MIT, of course.
