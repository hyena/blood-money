A World of Warcraft webapp that helps players determine the best value for their bloods of sargeras and primal sargerite on their realm.

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
  - Save data between runs and use it when bringing the service
    back up.
  - The threading model is presently fairly serial and could be
    improved such that it was hurt less by stragglers or one
    buggy realm.
  - There's definitely some major CPU usage when the download
    is running. Possibly some dumb deserialization issue or
    sorting.
  - We should probably move most of the work to the background
    thread and make the web thread just essentially take a reader
    lock and clone some `Arc`'s
  - Add some more crafting options now that we have the
    infrastructure for that.

Things that we might get to if this became more serious:
  - Currently does not respect changes in realm lists.
    Requires a restart to handle those changes.
  - Re-implement the `battle_net_api_client` into something
    robust: Use a modern version of hyper (means working with
    futures), flesh out all the method calls, move it into a
    lib.
  - Re-implement on rocket.rs instead of iron.

License
-------
Although I can't imagine someone else using this: MIT, of course.
