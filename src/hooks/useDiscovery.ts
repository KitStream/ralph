import { useState, useCallback } from "react";
import { discoverModes } from "../lib/commands";
import type { ModeInfo } from "../lib/types";

export function useDiscovery() {
  const [modes, setModes] = useState<ModeInfo[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const discover = useCallback(async (projectDir: string) => {
    setLoading(true);
    setError(null);
    try {
      const result = await discoverModes(projectDir);
      setModes(result);
    } catch (e) {
      setError(String(e));
      setModes([]);
    } finally {
      setLoading(false);
    }
  }, []);

  return { modes, loading, error, discover };
}
