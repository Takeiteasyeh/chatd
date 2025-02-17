# chatd
Rust Multi-threaded Irc-like webchat service -- This code was written to learn multi-threading and shared memory -- It is mostly, but not fully feature complete.

- You will need to install Rust from https://www.rust-lang.org/tools/install
- You will probably need gcc
- You will need to update chat.htm to use your hostname instead of the one in it
- You will want to generate an ssl certificate.
- Use 'cargo run, or cargo build' to generate or run the binary (from inside chatd)
- A configuration file will be dropped on first-run, you can edit this, otherwise, you may edit config.rs before build to start with sane defaults for your system.
- A admin password will be generated on first run.

  I WOULDN'T BOTHER WITH PULL REQUESTS, THIS PROJECT IS MOSTLY PROVIDED AS IS IN CURRENT STATE.
  I TRIED TO FOLLOW MOST BEST PRACTICES BUT THIS IS PROOF OF CONCEPT CODE.

  If you need help feel free to open an issue, but I probably wont be doing much in the way of bug or feature pushes (famous last words)
