import { useEffect, useState } from "react";

import { projectFileList } from "~/lib/ipc";

/**
 * Whether project `projectId` has files: `undefined` outside a project, and
 * `null` while not known or after a failed lookup. Looked up again whenever
 * `refresh` changes, such as when the user comes back from the project's
 * view, where files are added.
 */
const useHasFiles = (
  projectId: string | null,
  refresh: string
): boolean | null | undefined => {
  const key = `${projectId ?? ""}\n${refresh}`;
  const [found, setFound] = useState<{ key: string; hasFiles: boolean }>();

  useEffect(() => {
    if (projectId === null) {
      return undefined;
    }
    let cancelled = false;
    void projectFileList(projectId, 0, 1).then(
      (page) => {
        if (!cancelled) {
          setFound({ key, hasFiles: page.total > 0 });
        }
      },
      // Unknown: the Files switch stays reachable.
      () => undefined
    );
    return () => {
      cancelled = true;
    };
  }, [projectId, key]);

  if (projectId === null) {
    return undefined;
  }
  return found?.key === key ? found.hasFiles : null;
};

export { useHasFiles };
