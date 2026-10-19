import { appInfo } from "~/lib/ipc";

/**
 * The app version from the Rust side, or `""` when the command fails. The
 * version line is decoration, so a failure leaves it empty instead of
 * surfacing an error.
 */
const loadVersion = async (): Promise<string> => {
  try {
    const info = await appInfo();
    return info.version;
  } catch {
    return "";
  }
};

export { loadVersion };
