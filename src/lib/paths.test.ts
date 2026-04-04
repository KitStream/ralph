import { describe, it, expect } from "vitest";
import { worktreePrefix, shortenPaths, shortenAiBlock } from "./paths";

describe("worktreePrefix", () => {
  it("builds prefix from project dir and branch", () => {
    expect(worktreePrefix("/home/user/project", "my-branch"))
      .toBe("/home/user/project/.ralph/my-branch-worktree");
  });

  it("strips trailing slashes from project dir", () => {
    expect(worktreePrefix("/home/user/project/", "branch"))
      .toBe("/home/user/project/.ralph/branch-worktree");
  });

  it("normalizes backslashes", () => {
    expect(worktreePrefix("C:\\Users\\foo\\project", "branch"))
      .toBe("C:/Users/foo/project/.ralph/branch-worktree");
  });
});

describe("shortenPaths", () => {
  const prefix = "/home/user/project/.ralph/branch-worktree";

  it("returns text unchanged when prefix is empty", () => {
    expect(shortenPaths("hello", "")).toBe("hello");
  });

  it("returns empty string for empty text", () => {
    expect(shortenPaths("", prefix)).toBe("");
  });

  it("shortens prefix at start of path", () => {
    expect(shortenPaths("/home/user/project/.ralph/branch-worktree/src/main.rs", prefix))
      .toBe("\u2302/src/main.rs");
  });

  it("shortens exact match of prefix", () => {
    expect(shortenPaths(prefix, prefix)).toBe("\u2302");
  });

  it("shortens multiple occurrences", () => {
    const text = `cat ${prefix}/a.txt ${prefix}/b.txt`;
    expect(shortenPaths(text, prefix)).toBe("cat \u2302/a.txt \u2302/b.txt");
  });

  it("normalizes backslashes before matching", () => {
    const winPath = "C:\\Users\\foo\\project\\.ralph\\branch-worktree\\src\\main.rs";
    const winPrefix = "C:/Users/foo/project/.ralph/branch-worktree";
    expect(shortenPaths(winPath, winPrefix)).toBe("\u2302/src/main.rs");
  });

  it("returns text unchanged when prefix not found", () => {
    expect(shortenPaths("/other/path/file.rs", prefix)).toBe("/other/path/file.rs");
  });

  it("handles prefix as substring of longer dir name", () => {
    expect(shortenPaths("/home/user/project/.ralph/branch-worktree-extra/file.rs", prefix))
      .toBe("\u2302-extra/file.rs");
  });
});

describe("shortenAiBlock", () => {
  const prefix = "/home/user/project/.ralph/branch-worktree";

  it("shortens Read file_path", () => {
    const block = { kind: "ToolUse" as const, tool_id: "t1", tool: { tool: "Read" as const, file_path: `${prefix}/src/main.rs` } };
    const result = shortenAiBlock(block, prefix);
    expect(result.kind).toBe("ToolUse");
    if (result.kind === "ToolUse") {
      expect(result.tool.tool).toBe("Read");
      if (result.tool.tool === "Read") {
        expect(result.tool.file_path).toBe("\u2302/src/main.rs");
      }
    }
  });

  it("shortens Edit file_path but not old_string/new_string", () => {
    const block = {
      kind: "ToolUse" as const,
      tool_id: "t2",
      tool: { tool: "Edit" as const, file_path: `${prefix}/src/lib.rs`, old_string: `${prefix}/old`, new_string: `${prefix}/new` },
    };
    const result = shortenAiBlock(block, prefix);
    if (result.kind === "ToolUse" && result.tool.tool === "Edit") {
      expect(result.tool.file_path).toBe("\u2302/src/lib.rs");
      expect(result.tool.old_string).toBe(`${prefix}/old`);
      expect(result.tool.new_string).toBe(`${prefix}/new`);
    }
  });

  it("shortens Bash command", () => {
    const block = {
      kind: "ToolUse" as const,
      tool_id: "t3",
      tool: { tool: "Bash" as const, command: `ls ${prefix}/src`, description: null },
    };
    const result = shortenAiBlock(block, prefix);
    if (result.kind === "ToolUse" && result.tool.tool === "Bash") {
      expect(result.tool.command).toBe("ls \u2302/src");
    }
  });

  it("shortens Glob path but not pattern", () => {
    const block = {
      kind: "ToolUse" as const,
      tool_id: "t4",
      tool: { tool: "Glob" as const, pattern: "**/*.rs", path: `${prefix}/src` },
    };
    const result = shortenAiBlock(block, prefix);
    if (result.kind === "ToolUse" && result.tool.tool === "Glob") {
      expect(result.tool.pattern).toBe("**/*.rs");
      expect(result.tool.path).toBe("\u2302/src");
    }
  });

  it("leaves Text blocks unchanged", () => {
    const block = { kind: "Text" as const, text: `path: ${prefix}/foo` };
    const result = shortenAiBlock(block, prefix);
    expect(result).toBe(block);
  });

  it("leaves ToolResult blocks unchanged", () => {
    const block = { kind: "ToolResult" as const, tool_use_id: "t1", content: `${prefix}/foo`, is_error: false };
    const result = shortenAiBlock(block, prefix);
    expect(result).toBe(block);
  });

  it("handles Glob with null path", () => {
    const block = {
      kind: "ToolUse" as const,
      tool_id: "t5",
      tool: { tool: "Glob" as const, pattern: "*.rs", path: null },
    };
    const result = shortenAiBlock(block, prefix);
    if (result.kind === "ToolUse" && result.tool.tool === "Glob") {
      expect(result.tool.path).toBeNull();
    }
  });
});
