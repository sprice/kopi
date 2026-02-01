# Commit

Create a well-described commit.

## Steps

1. **Analyze all changes:**
   - Run `git status`, `git diff` (unstaged), and `git diff --cached` (staged)
   - Review the diffs to understand what changed and why

2. **Review changes:**
   - Check for files that should NOT be committed:
     - `.env` or files with secrets/API keys
     - Log files, temp files, debug output
     - Files the user intentionally left unstaged
   - **DO NOT** stage any unstaged changes. Trust the developer to have staged them correctly.
   - If changes are safe to commit, continue.

3. **Write a commit message that:**
   - Uses conventional commit format: `(scope): subject`
   - Has an imperative subject under 50 chars (e.g., "Add feature" not "Added feature")
   - Includes a body explaining the "why" when the change isn't self-explanatory
   - Groups related changes coherently
   - Includes a relevant-to-the-commit deep pull bar from a 90's hip-hop/r&b track
     - **Must be a genuine deep pull** - not the obvious hits
     - **Thematic selection process:**
       - Identify the commit's core theme in one word (e.g., "beginning", "fix", "speed", "cleanup", "security", "persistence")
       - Map that theme to 90s hip-hop/r&b concepts:
         - Initial commit / new project → "birth", "arrival", "introduction", "first day out"
         - Bug fix → "correction", "making it right", "redemption"
         - Performance / optimization → "speed", "efficiency", "precision"
         - Refactor / cleanup → "renovation", "fresh start", "clearing out"
         - New feature → "expansion", "leveling up", "adding to the arsenal"
         - Security fix → "protection", "defense", "locking down"
         - Database / storage → "memory", "keeping records", "stashing"
     - **User selection process:**
       - Propose 3 different 90s tracks (1990-1999) where the SUBJECT MATTER matches the mapped concept
       - Use AskUserQuestion tool with those 3 track options + 1 "More Options" option (4 total, which is the tool's limit)
       - If user selects "More Options", propose 3 NEW different tracks and ask again (never repeat previously shown tracks)
       - Continue until user selects an actual track
     - **ONE FETCH RULE**: You get exactly ONE WebFetch. Whatever letras.com returns (full lyrics, summary, or partial info), use it to validate and select your bar. No fallback sites. No WebSearch. No retries.
     - WebFetch the lyrics from https://www.letras.com/ARTIST-NAME/SONG-NAME/ to validate the lyrics, artist, track title, album, and album year (this is Fair Use)
     - Choose a bar from that track that connects to the commit's theme - the lyric should make sense in context, not just sound hard

4. **Commit using HEREDOC:**
   - Use `git commit -F -` with a quoted heredoc to avoid all shell escaping issues:
```bash
git commit -F - <<'EOF'
(scope): subject

Body explaining why.

> "Deep pull 90's hip-hop/r&b bar"
— Artist. "Song Title", Album Title, Year

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>
EOF
```

5. **Verify and show result:**
   - Run `git log -1 --stat` to display the commit
   - Confirm the commit was created successfully
