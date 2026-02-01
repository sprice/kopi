# Code Review Command

Perform the type of critical code review an expert Rust software engineer and expert Rust software architect would do.

**IMPORTANT** Follow the coding standards and best practices outlined in the `CLAUDE.md` file for this review.

## About This App

It is important to understand that this is a simple single-user clipboard management application.

## Steps

**IMPORTANT** Use three Opus sub-agents closely collaborating with each other for the following work. These sub-agents are experts in software engineering, software architecture, and software testing and quality assurance.

**IMPORTANT** Analyze code files only. Do not refer to any documentation that describes how the code should work. Code is the source of truth.

1. **Check Diff**

- Check the diff between the current branch against `main`

2. **Analyze each file thoroughly**

- For every file in the diff:
  - Read the full file content if it's a new/modified file to understand context
  - Examine the specific changes line by line from the diff
  - Check against project coding standards from CLAUDE.md
    - All coding standards are important
  - Identify potential issues:
    - Security vulnerabilities or exposed sensitive data
    - Performance bottlenecks or inefficiencies
    - Logic bugs or edge cases not handled
    - Code maintainability and readability concerns
    - Missing error handling or validation
    - Breaking changes that affect other parts of the codebase
  - For each issue found, note the specific file path and line number references
- Assess the broader impact: how changes in each file affect related components,
  APIs, database schemas, or user workflows

3. **Create comprehensive review**

- Write a complete and accurate code review document that covers:
  - **Executive Summary**: Brief overview of changes, risk level, and key
    concerns
  - **Files Changed**: List all modified files with change summary
  - **Critical Issues**: Security, breaking changes, or major bugs requiring
    immediate attention
  - **Detailed Analysis**: For each file with issues:
    - `### path/to/file.ext`
    - **Changes**: What was modified
    - **Issues Found**: Specific problems with file:line references
    - **Recommendations**: Actionable fixes with code examples where helpful
    - **Impact**: How changes affect other parts of the system
  - **Overall Assessment**: System-wide impact, testing recommendations,
    deployment considerations
  - **Action Items**: Prioritized checklist of required fixes and improvements

Be constructive and helpful in your feedback.
