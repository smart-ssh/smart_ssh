### Changed
- Redirections that write nothing (`2>/dev/null`, `>/dev/null`, `&>/dev/null`,
  `2>&1`, `>&2`) no longer force a confirmation. Other redirections still do.
