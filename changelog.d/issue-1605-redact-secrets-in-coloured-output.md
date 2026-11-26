### Security
- Secrets in command output that contains terminal colour or cursor codes (for example `grep --color=always`) are now masked before the text reaches the AI, the chat or logs. The codes are removed from that text.
