### Changed
- If a local Ollama model without tool support causes an error, the app now
  shows the message "This model does not support tool calling" with a hint to
  untick "Supports native tool calling" in the provider settings, instead of
  a generic provider error.
