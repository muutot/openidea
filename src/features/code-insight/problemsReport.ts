/** Reports changed files from the Problems view to the built-in MCP host. */
import { commands, call, type FileProblems } from "@/platform/ipc";
import { useProject } from "@/platform/project";
import { useRemote } from "@/platform/remote";
import { useProblems, type ProblemsMap } from "./problemsStore";

const REPORT_MS = 1000;

export function reportProblems(): () => void {
  let disposed = false;
  let reported: ProblemsMap = new Map();
  let reset = true;
  let timer: ReturnType<typeof setTimeout> | null = null;
  const flush = () => {
    timer = null;
    if (!useProject.getState().project) return;
    const map = useProblems.getState().map;
    const files: FileProblems[] = [];
    for (const [path, sources] of map) {
      if (!reset && reported.get(path) === sources) continue;
      // Per producer: the host records diagnostics under the producer's name, so a
      // file's items must arrive split, or one producer's clear wipes the others.
      for (const [, items] of sources) files.push({ path, items: [...items] });
      if (!sources.size) files.push({ path, items: [] });
    }
    if (!reset)
      for (const [path, sources] of reported)
        if (!map.has(path)) for (const _source of sources.keys()) files.push({ path, items: [] });
    if (!files.length && !reset) return;
    const wasReset = reset;
    reported = map;
    reset = false;
    void call(commands.mcpReportProblems(wasReset, files)).catch(() => {
      reset = true;
      if (!disposed) schedule();
    });
  };
  const schedule = () => {
    if (!timer) timer = setTimeout(flush, REPORT_MS);
  };
  schedule();
  const off = useProblems.subscribe((s, prev) => {
    if (s.map !== prev.map) schedule();
  });
  const offProject = useProject.subscribe((s, prev) => {
    if (s.project?.root !== prev.project?.root) {
      reset = true;
      reported = new Map();
      schedule();
    }
  });
  const offRemote = useRemote.subscribe((s, prev) => {
    if (s.session !== prev.session) {
      reset = true;
      reported = new Map();
      schedule();
    }
  });
  return () => {
    disposed = true;
    offRemote();
    offProject();
    off();
    if (timer) clearTimeout(timer);
  };
}
