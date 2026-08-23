---
description: "Use when handling UI, design, style, or minor visual-polish tasks. Skip build verification for these changes and rely on the running dev server and user-reported compile issues."
name: "UI Style Changes: Skip Build Verification"
---
# UI Style and Minor UX Workflow

- For UI/design/style work and other minor, non-foundational UI changes, do not run build verification commands.
- Do not run commands like `pnpm build`, `npm run build`, `ng build`, or equivalent full compile checks for these tasks.
- Assume a dev server is likely already running and rely on user feedback for real compile/runtime issues.
- Continue validating by inspecting the code changes and, when useful, checking behavior in the running UI.
- If the user explicitly asks for a build, run it.
- If the task is a major new UI feature (not minor polish), build verification is allowed when it is clearly useful.
