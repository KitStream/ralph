import { useRef, useEffect, useCallback } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import type { LogEntry, LogCategory } from "../lib/types";

const categoryColorVars: Record<LogCategory, string> = {
  Git: "var(--log-git)",
  Ai: "var(--log-ai)",
  Script: "var(--log-script)",
  Warning: "var(--log-warning)",
  Error: "var(--log-error)",
};

interface LogViewProps {
  logs: LogEntry[];
}

export function LogView({ logs }: LogViewProps) {
  const parentRef = useRef<HTMLDivElement>(null);
  const autoScrollRef = useRef(true);

  const virtualizer = useVirtualizer({
    count: logs.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 20,
    overscan: 50,
    measureElement: (el) => el.getBoundingClientRect().height,
  });

  // Auto-scroll to bottom when new logs arrive
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
                color: categoryColorVars[log.category],
                whiteSpace: "pre-wrap",
                wordBreak: "break-all",
                paddingBottom: 2,
              }}
            >
              {log.text}
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
