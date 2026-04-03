import { useRef, useEffect, useCallback, useState } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import Markdown from "react-markdown";
import type {
  LogEntry,
  LogCategory,
  AiContentBlock,
  HousekeepingBlock,
  ToolInvocation,
} from "../lib/types";

const categoryColorVars: Record<LogCategory, string> = {
  Git: "var(--log-git)",
  Ai: "var(--log-ai)",
  Script: "var(--log-script)",
  Warning: "var(--log-warning)",
  Error: "var(--log-error)",
};

interface LogViewProps {
  logs: LogEntry[];
  projectDir?: string;
  branchName?: string;
  rateLimitMessage?: string | null;
}

export function LogView({ logs, projectDir, branchName, rateLimitMessage }: LogViewProps) {
  const worktreePrefix = projectDir && branchName
    ? `${projectDir}/.ralph/${branchName}-worktree`
    : undefined;
  const parentRef = useRef<HTMLDivElement>(null);
  const autoScrollRef = useRef(true);

  const virtualizer = useVirtualizer({
    count: logs.length,
    getScrollElement: () => parentRef.current,
    estimateSize: (index) => {
      const log = logs[index];
      if (log?.aiBlock) {
        switch (log.aiBlock.kind) {
          case "Text": return 20 * Math.min(log.aiBlock.text.split("\n").length, 5);
          case "ToolUse": return 32;
          case "ToolResult": return 24;
        }
      }
      if (log?.housekeepingBlock) return 28;
      return 20;
    },
    overscan: 50,
    measureElement: (el) => el.getBoundingClientRect().height,
  });

  useEffect(() => {
    if (autoScrollRef.current && logs.length > 0) {
      virtualizer.scrollToIndex(logs.length - 1, { align: "end" });
    }
  }, [logs.length, virtualizer]);

  const handleScroll = useCallback(() => {
    const el = parentRef.current;
    if (!el) return;
    const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 50;
    autoScrollRef.current = atBottom;
  }, []);

  return (
    <div
      ref={parentRef}
      onScroll={handleScroll}
      style={{
        flex: 1,
        overflow: "auto",
        backgroundColor: "var(--bg-primary)",
        fontFamily: "'JetBrains Mono', 'Fira Code', monospace",
        fontSize: "13px",
        lineHeight: "20px",
        padding: "8px",
      }}
    >
      {rateLimitMessage && (
        <div
          style={{
            position: "sticky",
            top: 0,
            zIndex: 10,
            backgroundColor: "#78350f",
            color: "#fef3c7",
            padding: "8px 12px",
            borderRadius: 4,
            margin: "0 0 8px",
            display: "flex",
            alignItems: "center",
            gap: 8,
            fontSize: "13px",
            fontWeight: 500,
            animation: "pulse 2s ease-in-out infinite",
          }}
        >
          <span style={{ fontSize: "16px" }}>⏸</span>
          <span>{rateLimitMessage}</span>
          <span style={{ color: "#fcd34d", fontSize: "12px", marginLeft: "auto" }}>
            Waiting for limit to reset...
          </span>
        </div>
      )}
      <div
        style={{
          height: virtualizer.getTotalSize(),
          width: "100%",
          position: "relative",
        }}
      >
        {virtualizer.getVirtualItems().map((virtualRow) => {
          const log = logs[virtualRow.index];
          return (
            <div
              key={virtualRow.key}
              ref={virtualizer.measureElement}
              data-index={virtualRow.index}
              style={{
                position: "absolute",
                top: 0,
                left: 0,
                width: "100%",
                transform: `translateY(${virtualRow.start}px)`,
                paddingBottom: 2,
              }}
            >
              <LogRow log={log} worktreePrefix={worktreePrefix} />
            </div>
          );
        })}
      </div>
      {logs.length === 0 && (
        <div style={{ color: "var(--text-muted)", fontStyle: "italic", padding: "16px" }}>
          No log output yet. Start the session to begin.
        </div>
      )}
    </div>
  );
}

function shortenPath(path: string, worktreePrefix?: string): string {
  if (worktreePrefix && path.startsWith(worktreePrefix)) {
    return "⌂" + path.slice(worktreePrefix.length);
  }
  return path;
}

/** Replace all occurrences of the worktree prefix in arbitrary text (e.g. bash commands). */
function shortenText(text: string, worktreePrefix?: string): string {
  if (!worktreePrefix) return text;
  return text.split(worktreePrefix).join("⌂");
}

function LogRow({ log, worktreePrefix }: { log: LogEntry; worktreePrefix?: string }) {
  if (log.aiBlock) {
    return <AiBlockRow block={log.aiBlock} worktreePrefix={worktreePrefix} />;
  }
  if (log.housekeepingBlock) {
    return <HousekeepingRow block={log.housekeepingBlock} />;
  }
  return (
    <div
      style={{
        color: categoryColorVars[log.category],
        whiteSpace: "pre-wrap",
        wordBreak: "break-all",
      }}
    >
      {log.text}
    </div>
  );
}

// --- AI Content Blocks ---

function AiBlockRow({ block, worktreePrefix }: { block: AiContentBlock; worktreePrefix?: string }) {
  switch (block.kind) {
    case "Text":
      return <AiTextBlock text={block.text} />;
    case "ToolUse":
      return <ToolUseBlock tool={block.tool} worktreePrefix={worktreePrefix} />;
    case "ToolResult":
      return <ToolResultBlock content={block.content} isError={block.is_error} />;
  }
}

function AiTextBlock({ text }: { text: string }) {
  return (
    <div style={{ color: "var(--log-ai)" }} className="ai-markdown">
      <Markdown>{text}</Markdown>
    </div>
  );
}

const toolColors: Record<string, string> = {
  Read: "#60a5fa",    // blue
  Edit: "#fbbf24",    // yellow
  Write: "#fbbf24",   // yellow
  Bash: "#22d3ee",    // cyan
  Glob: "#60a5fa",    // blue
  Grep: "#60a5fa",    // blue
  Other: "#a78bfa",   // purple
};

function ToolUseBlock({ tool, worktreePrefix }: { tool: ToolInvocation; worktreePrefix?: string }) {
  const color = toolColors[tool.tool] ?? toolColors.Other;

  return (
    <div style={{ borderLeft: `2px solid ${color}`, paddingLeft: 8, margin: "2px 0" }}>
      {renderToolHeader(tool, color, worktreePrefix)}
      {renderToolDetail(tool)}
    </div>
  );
}

function renderToolHeader(tool: ToolInvocation, color: string, worktreePrefix?: string) {
  const badge = (label: string) => (
    <span
      style={{
        backgroundColor: color,
        color: "#000",
        padding: "1px 6px",
        borderRadius: 3,
        fontSize: "11px",
        fontWeight: 600,
        marginRight: 6,
      }}
    >
      {label}
    </span>
  );
  const fp = (path: string) => shortenPath(path, worktreePrefix);

  switch (tool.tool) {
    case "Read":
      return <div>{badge("Read")}<span style={{ color: "#e2e8f0" }}>{fp(tool.file_path)}</span></div>;
    case "Edit":
      return <div>{badge("Edit")}<span style={{ color: "#e2e8f0" }}>{fp(tool.file_path)}</span></div>;
    case "Write":
      return <div>{badge("Write")}<span style={{ color: "#e2e8f0" }}>{fp(tool.file_path)}</span></div>;
    case "Bash":
      return (
        <div>
          {badge("$")}
          <span style={{ color: "#e2e8f0" }}>{shortenText(tool.command, worktreePrefix)}</span>
          {tool.description && <span style={{ color: "#94a3b8", marginLeft: 8, fontSize: "12px" }}>{tool.description}</span>}
        </div>
      );
    case "Glob":
      return <div>{badge("Glob")}<span style={{ color: "#e2e8f0" }}>{tool.pattern}</span>{tool.path && <span style={{ color: "#94a3b8" }}> in {fp(tool.path)}</span>}</div>;
    case "Grep":
      return <div>{badge("Grep")}<span style={{ color: "#e2e8f0" }}>{tool.pattern}</span>{tool.path && <span style={{ color: "#94a3b8" }}> in {fp(tool.path)}</span>}</div>;
    case "Other":
      return <div>{badge(tool.name)}</div>;
  }
}

function renderToolDetail(tool: ToolInvocation) {
  if (tool.tool === "Edit") {
    return (
      <div style={{ marginTop: 4, fontSize: "12px" }}>
        {tool.old_string.split("\n").map((line, i) => (
          <div key={`old-${i}`} style={{ backgroundColor: "rgba(239,68,68,0.15)", color: "#fca5a5", whiteSpace: "pre" }}>
            <span style={{ userSelect: "none", color: "#ef4444" }}>- </span>{line}
          </div>
        ))}
        {tool.new_string.split("\n").map((line, i) => (
          <div key={`new-${i}`} style={{ backgroundColor: "rgba(34,197,94,0.15)", color: "#86efac", whiteSpace: "pre" }}>
            <span style={{ userSelect: "none", color: "#22c55e" }}>+ </span>{line}
          </div>
        ))}
      </div>
    );
  }
  return null;
}

function ToolResultBlock({ content, isError }: { content: string; isError: boolean }) {
  const [expanded, setExpanded] = useState(false);
  const lines = content.split("\n");
  const isLong = lines.length > 5;
  const displayLines = expanded ? lines : lines.slice(0, 5);

  if (!content.trim()) return null;

  return (
    <div
      style={{
        paddingLeft: 10,
        color: isError ? "var(--log-error)" : "#94a3b8",
        fontSize: "12px",
        cursor: isLong ? "pointer" : undefined,
      }}
      onClick={isLong ? () => setExpanded(!expanded) : undefined}
    >
      {displayLines.map((line, i) => (
        <div key={i} style={{ whiteSpace: "pre-wrap", wordBreak: "break-all" }}>{line}</div>
      ))}
      {isLong && !expanded && (
        <div style={{ color: "#64748b", fontStyle: "italic" }}>
          ... {lines.length - 5} more lines (click to expand)
        </div>
      )}
      {isLong && expanded && (
        <div style={{ color: "#64748b", fontStyle: "italic" }}>
          (click to collapse)
        </div>
      )}
    </div>
  );
}

// --- Housekeeping Blocks ---

function HousekeepingRow({ block }: { block: HousekeepingBlock }) {
  switch (block.kind) {
    case "StepStarted":
      return (
        <div style={{ color: "var(--log-git)", display: "flex", alignItems: "center", gap: 6 }}>
          <span style={{ opacity: 0.7 }}>▸</span>
          <StepBadge step={block.step} />
          <span>{block.description}</span>
        </div>
      );
    case "StepCompleted":
      return (
        <div style={{ color: "#4ade80", display: "flex", alignItems: "center", gap: 6 }}>
          <span>✓</span>
          <StepBadge step={block.step} />
          <span>{block.summary}</span>
        </div>
      );
    case "GitCommand":
      if (!block.output.trim()) return null;
      return (
        <div style={{ color: "var(--log-git)", whiteSpace: "pre-wrap", wordBreak: "break-all", paddingLeft: 10 }}>
          {block.output}
        </div>
      );
    case "DiffStat":
      return (
        <div style={{ color: "var(--log-git)", whiteSpace: "pre-wrap", paddingLeft: 10 }}>
          {block.stat}
        </div>
      );
    case "Recovery":
      return (
        <div style={{ color: "var(--log-warning)", display: "flex", alignItems: "center", gap: 6 }}>
          <span>↻</span>
          <span style={{ fontWeight: 600 }}>{block.action}:</span>
          <span>{block.detail}</span>
        </div>
      );
  }
}

const stepLabels: Record<string, string> = {
  Idle: "idle",
  Checkout: "checkout",
  RebasePreAi: "rebase",
  RunningAi: "ai",
  PushBranch: "push",
  RebasePostAi: "rebase",
  PushToMain: "push-main",
  Tagging: "tag",
  RecoveringGit: "recovery",
  Paused: "paused",
};

function StepBadge({ step }: { step: string }) {
  return (
    <span
      style={{
        backgroundColor: "rgba(86,212,221,0.2)",
        color: "var(--log-git)",
        padding: "0 5px",
        borderRadius: 3,
        fontSize: "11px",
        fontWeight: 500,
      }}
    >
      {stepLabels[step] ?? step}
    </span>
  );
}
