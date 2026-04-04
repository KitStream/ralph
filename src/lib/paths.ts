import type { AiContentBlock } from "./types";

export function worktreePrefix(projectDir: string, branchName: string): string {
  const dir = projectDir.replace(/\\/g, "/").replace(/\/+$/, "");
  return `${dir}/.ralph/${branchName}-worktree`;
}

export function shortenPaths(text: string, prefix: string): string {
  if (!prefix) return text;
  const norm = text.replace(/\\/g, "/");
  return norm.split(prefix).join("\u2302");
}

export function shortenAiBlock(block: AiContentBlock, prefix: string): AiContentBlock {
  if (block.kind !== "ToolUse") return block;
  const sp = (p: string) => shortenPaths(p, prefix);
  const t = block.tool;
  let shortTool = t;
  switch (t.tool) {
    case "Read": shortTool = { ...t, file_path: sp(t.file_path) }; break;
    case "Edit": shortTool = { ...t, file_path: sp(t.file_path) }; break;
    case "Write": shortTool = { ...t, file_path: sp(t.file_path) }; break;
    case "Bash": shortTool = { ...t, command: sp(t.command) }; break;
    case "Glob": shortTool = { ...t, path: t.path ? sp(t.path) : null }; break;
    case "Grep": shortTool = { ...t, path: t.path ? sp(t.path) : null }; break;
  }
  return { ...block, tool: shortTool };
}
