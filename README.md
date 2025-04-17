# from me
- fork of stocko's aido project
    - refactored as a clap-based CLI for better usage
    - added some functionality:
        - "--dry-run" option for testing purposes
        - multiple shell options (powershell and bash) rather than just bash
        - cross platform config using 'dirs' (XDG/AppData)
        - changed default model to gemini-2.0-flash (free tier limits on 2.0 flash is more than enough)

# aido
"prompt to command one-liner" ai cli tool for the terminal

**Example:**
```bash
aido add all unstaged changes and commit with message 'demo'
```
