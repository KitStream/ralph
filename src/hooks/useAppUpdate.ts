import { useCallback, useEffect, useRef, useState } from "react";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { getVersion } from "@tauri-apps/api/app";

export type UpdateStatus =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "upToDate" }
  | { kind: "available"; version: string; notes: string | null }
  | { kind: "downloading"; downloaded: number; total: number | null }
  | { kind: "ready" }
  | { kind: "error"; message: string };

export function useAppUpdate() {
  const [version, setVersion] = useState<string>("");
  const [status, setStatus] = useState<UpdateStatus>({ kind: "idle" });
  const pendingUpdate = useRef<Update | null>(null);

  useEffect(() => {
    getVersion().then(setVersion).catch(() => {});
  }, []);

  const checkForUpdate = useCallback(async (): Promise<void> => {
    setStatus({ kind: "checking" });
    try {
      const update = await check();
      if (update) {
        pendingUpdate.current = update;
        setStatus({
          kind: "available",
          version: update.version,
          notes: update.body ?? null,
        });
      } else {
        pendingUpdate.current = null;
        setStatus({ kind: "upToDate" });
      }
    } catch (err) {
      setStatus({ kind: "error", message: String(err) });
    }
  }, []);

  const install = useCallback(async (): Promise<void> => {
    const update = pendingUpdate.current;
    if (!update) return;
    try {
      let downloaded = 0;
      let total: number | null = null;
      setStatus({ kind: "downloading", downloaded: 0, total: null });
      await update.downloadAndInstall((event) => {
        if (event.event === "Started") {
          total = event.data.contentLength ?? null;
          setStatus({ kind: "downloading", downloaded: 0, total });
        } else if (event.event === "Progress") {
          downloaded += event.data.chunkLength;
          setStatus({ kind: "downloading", downloaded, total });
        } else if (event.event === "Finished") {
          setStatus({ kind: "ready" });
        }
      });
      await relaunch();
    } catch (err) {
      setStatus({ kind: "error", message: String(err) });
    }
  }, []);

  const dismiss = useCallback(() => {
    setStatus({ kind: "idle" });
  }, []);

  return { version, status, checkForUpdate, install, dismiss };
}
