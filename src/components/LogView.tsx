import { useRef, useEffect, useCallback, useMemo, useState } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import Markdown from "react-markdown";
import type {
  LogEntry,
  LogCategory,
  AiContentBlock,
  HousekeepingBlock,
  ToolInvocation,
  ToolResultData,
  IterationSummary,
} from "../lib/types";

const categoryColorVars: Record<LogCategory, string> = {
  Git: "var(--log-git)",
  Ai: "var(--log-ai)",
  Script: "var(--log-script)",
  Warning: "var(--log-warning)",
  Error: "var(--log-error)",
};

type DisplayItem =
  | { type: "iteration-header"; iteration: number; entryCount: number; folded: boolean }
  | { type: "log-entry"; entry: LogEntry };

function buildDisplayList(
  iterations: IterationSummary[],
  iterationLogs: Map<number, LogEntry[]>,
  foldedIterations: Set<number>,
): DisplayItem[] {
  const items: DisplayItem[] = [];
  for (const iter of iterations) {
    const folded = foldedIterations.has(iter.iteration);
    items.push({
      type: "iteration-header",
      iteration: iter.iteration,
      entryCount: iter.entry_count,
      folded,
    });
    if (!folded) {
      const entries = iterationLogs.get(iter.iteration);
      if (entries) {
        for (const entry of entries) {
          items.push({ type: "log-entry", entry });
        }
      }
    }
  }
  return items;
}

interface LogViewProps {
  iterations: IterationSummary[];
  iterationLogs: Map<number, LogEntry[]>;
  foldedIterations: Set<number>;
  onToggleFold: (iteration: number) => void;
  projectDir?: string;
  branchName?: string;
  showToolOutput?: boolean;
  toolOutputPreviewLines?: number;
  rateLimitMessage?: string | null;
}

export function LogView({
  iterations,
  iterationLogs,
  foldedIterations,
  onToggleFold,
  projectDir,
  branchName,
  showToolOutput = true,
  toolOutputPreviewLines = 2,
  rateLimitMessage,
}: LogViewProps) {
  const worktreePrefix = projectDir && branchName
    ? `${projectDir}/.ralph/${branchName}-worktree`
    : undefined;

  const parentRef = useRef<HTMLDivElement>(null);
  const autoScrollRef = useRef(true);

  const displayList = useMemo(
    () => buildDisplayList(iterations, iterationLogs, foldedIterations),
    [iterations, iterationLogs, foldedIterations],
  );

  const virtualizer = useVirtualizer({
    count: displayList.length,
    getScrollElement: () => parentRef.current,
    estimateSize: (index) => {
      const item = displayList[index];
      if (!item) return 20;
      if (item.type === "iteration-header") return 32;
      const log = item.entry;
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

  const totalEntries = useMemo(() => {
    let count = 0;
    for (const iter of iterations) count += iter.entry_count;
    return count;
  }, [iterations]);

  useEffect(() => {
    if (autoScrollRef.current && displayList.length > 0) {
      virtualizer.scrollToIndex(displayList.length - 1, { align: "end" });
    }
  }, [totalEntries, displayList.length, virtualizer]);

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
          const item = displayList[virtualRow.index];
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
              {item.type === "iteration-header" ? (
                <IterationHeader
                  iteration={item.iteration}
                  entryCount={item.entryCount}
                  folded={item.folded}
                  onToggle={() => onToggleFold(item.iteration)}
                />
              ) : (
                <LogRow log={item.entry} worktreePrefix={worktreePrefix} toolOutputPreviewLines={toolOutputPreviewLines} showToolOutput={showToolOutput} />
              )}
            </div>
          );
        })}
      </div>
      {displayList.length === 0 && (
        <div style={{ color: "var(--text-muted)", fontStyle: "italic", padding: "16px" }}>
          No log output yet. Start the session to begin.
        </div>
      )}
    </div>
  );
}

function IterationHeader({
  iteration,
  entryCount,
  folded,
  onToggle,
}: {
  iteration: number;
  entryCount: number;
  folded: boolean;
  onToggle: () => void;
}) {
  return (
    <div
      onClick={onToggle}
      style={{
        display: "flex",
        alignItems: "center",
        gap: 8,
        padding: "4px 8px",
        backgroundColor: "var(--bg-tertiary)",
        borderRadius: 4,
        cursor: "pointer",
        fontSize: "12px",
        color: "var(--text-secondary)",
        userSelect: "none",
      }}
    >
      <span style={{ fontSize: "10px", width: 12 }}>{folded ? "▸" : "▾"}</span>
      <span style={{ fontWeight: 600, color: "var(--text-primary)" }}>
        Iteration {iteration}
      </span>
      <span style={{ color: "var(--text-muted)" }}>
        {entryCount} {entryCount === 1 ? "entry" : "entries"}
      </span>
      {folded && !entryCount && (
        <span style={{ color: "var(--text-muted)", fontStyle: "italic" }}>empty</span>
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

function shortenText(text: string, worktreePrefix?: string): string {
  if (!worktreePrefix) return text;
  return text.split(worktreePrefix).join("⌂");
}

function LogRow({ log, worktreePrefix, toolOutputPreviewLines, showToolOutput = true }: { log: LogEntry; worktreePrefix?: string; toolOutputPreviewLines?: number; showToolOutput?: boolean }) {
  if (log.aiBlock) {
    return <AiBlockRow block={log.aiBlock} toolResult={showToolOutput ? log.toolResult : undefined} worktreePrefix={worktreePrefix} toolOutputPreviewLines={toolOutputPreviewLines} />;
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

function AiBlockRow({ block, toolResult, worktreePrefix, toolOutputPreviewLines }: { block: AiContentBlock; toolResult?: ToolResultData; worktreePrefix?: string; toolOutputPreviewLines?: number }) {
  switch (block.kind) {
    case "Text":
      return <AiTextBlock text={block.text} />;
    case "ToolUse":
      return <ToolUseBlock tool={block.tool} toolResult={toolResult} worktreePrefix={worktreePrefix} toolOutputPreviewLines={toolOutputPreviewLines} />;
    case "ToolResult":
      // Standalone fallback (if no matching ToolUse was found)
      return <ToolResultBlock content={block.content} isError={block.is_error} previewLines={toolOutputPreviewLines} />;
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
  Read: "#60a5fa",
  Edit: "#fbbf24",
  Write: "#fbbf24",
  Bash: "#22d3ee",
  Glob: "#60a5fa",
  Grep: "#60a5fa",
  Other: "#a78bfa",
};

function ToolUseBlock({ tool, toolResult, worktreePrefix, toolOutputPreviewLines }: { tool: ToolInvocation; toolResult?: ToolResultData; worktreePrefix?: string; toolOutputPreviewLines?: number }) {
  const color = toolColors[tool.tool] ?? toolColors.Other;
  return (
    <div style={{ borderLeft: `2px solid ${color}`, paddingLeft: 8, margin: "2px 0" }}>
      {renderToolHeader(tool, color, worktreePrefix)}
      {renderToolDetail(tool)}
      {toolResult && (
        <ToolResultBlock content={toolResult.content} isError={toolResult.is_error} previewLines={toolOutputPreviewLines} />
      )}
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
    case "Other": {
      // Show a compact summary of the input arguments
      const summary = Object.entries(tool.input)
        .filter(([, v]) => typeof v === "string" || typeof v === "number")
        .map(([k, v]) => `${k}: ${String(v).slice(0, 80)}`)
        .join(", ");
      return <div>{badge(tool.name)}{summary && <span style={{ color: "#94a3b8" }}> {summary}</span>}</div>;
    }
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

function ToolResultBlock({ content, isError, previewLines = 2 }: { content: string; isError: boolean; previewLines?: number }) {
  const [expanded, setExpanded] = useState(false);
  const lines = content.split("\n");
  const isLong = lines.length > previewLines;
  const displayLines = expanded ? lines : lines.slice(0, previewLines);

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
          ... {lines.length - previewLines} more lines (click to expand)
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
