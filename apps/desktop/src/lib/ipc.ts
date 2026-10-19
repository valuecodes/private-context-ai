// The only module that imports `@tauri-apps/api`. Everything else calls these
// typed wrappers, so `vi.mock("~/lib/ipc")` is the single test seam.
import { invoke } from "@tauri-apps/api/core";

import type { AppInfo } from "~/lib/types";

const appInfo = (): Promise<AppInfo> => invoke<AppInfo>("app_info");

export { appInfo };
