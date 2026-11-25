### Security
- A command hidden behind a backslash, a `#` comment or a line continuation is
  now seen by deny rules and the hard blacklist; comments and continuations ask.
- A command inside a substitution is read with its own quotes and comments,
  also within double quotes, and `${ cmd; }` counts as a substitution.
