# Security policy

omalaunch runs downloaded executables on your machine, so this is taken
seriously — but note what omalaunch is: a local-first launcher. There are
no accounts, no servers, no telemetry, and the only network use is
AppImage updates you explicitly trigger.

## Reporting a vulnerability

**Do not open a public issue.** Email acxd@tutamail.com with:

- what you found and why it matters (threat model in one paragraph),
- exact reproduction steps,
- the version/commit and your environment.

You will get a first response within 7 days. Please give a reasonable
window to fix before disclosing publicly.

## Scope notes

- AppImages themselves are untrusted third-party binaries. omalaunch never
  executes one during parsing (SquashFS is read as data), and launching
  always goes through your explicit action — but running an AppImage is
  inherently running someone else's code.
- Update downloads are length-verified and swapped atomically, but zsync
  channels are only as trustworthy as the publisher's hosting.
- Out of scope: vulnerabilities in AppImages, GTK, or the OS itself —
  report those upstream.
