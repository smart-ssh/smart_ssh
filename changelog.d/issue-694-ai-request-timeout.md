### Fixed
- An AI request that never answers (for example when the provider only sends keep-alive signals) now stops after 120 seconds of silence, or 10 minutes in total, with an error naming the provider and a "Try again" button.

### Changed
- While waiting for an AI reply, the chat shows which provider it is waiting for and how long, with a Stop button.
