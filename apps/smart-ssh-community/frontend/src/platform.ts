import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

/**
 * Platform detection. This is the only production module that reads
 * `navigator.platform` / `navigator.userAgent` (guarded by `platformGuard.test.ts`).
 */

export type Platform = "macos" | "windows" | "linux" | "android" | "unknown";

/** Spec 0114: what the platform offers beyond the shared UI. Android has no
 * local shell, no folder picker and no system file clipboard. */
export interface PlatformCapabilities {
  /** The local pseudo-server entry in the server list. */
  localServer: boolean;
  /** "Upload folder" and "Download to…" for folders (native folder picker). */
  folderPicker: boolean;
  /** "Copy file" and pasting local files from the system clipboard. */
  fileClipboard: boolean;
  /** Always the narrow (phone) layout, whatever the window width. */
  alwaysNarrow: boolean;
  /** "Reveal in folder" after a download (a file manager window). */
  revealInFolder: boolean;
  /** Dropping files from the OS onto the file browser to upload them. */
  dragDrop: boolean;
  /** Opening an edit copy in an external program; without it the app's own
   * editor view is used (issue #679). */
  externalEditor: boolean;
  /** Download straight into the default folder; without it every download
   * goes through the system save picker (issue #679). */
  downloadWithoutPicker: boolean;
  /** Keeping the path of a key file instead of importing the key. */
  keyFilePath: boolean;
}

export function platformCapabilities(platform: Platform): PlatformCapabilities {
  const android = platform === "android";
  return {
    localServer: !android,
    folderPicker: !android,
    fileClipboard: !android,
    alwaysNarrow: android,
    revealInFolder: !android,
    dragDrop: !android,
    externalEditor: !android,
    downloadWithoutPicker: !android,
    keyFilePath: !android,
  };
}

export function detectFallbackPlatform(): Platform {
  if (typeof navigator === "undefined") return "unknown";
  const ua = navigator.userAgent.toLowerCase();
  // Android WebViews also say "linux": test android first.
  if (ua.includes("android")) return "android";
  if (ua.includes("mac")) return "macos";
  if (ua.includes("win")) return "windows";
  if (ua.includes("linux")) return "linux";
  return "unknown";
}

/** Plattform über `get_platform` (auch vor der Entsperrung erlaubt,
 * Spec 0101 A16), bis zur Antwort bzw. bei einem Fehler aus dem
 * User-Agent geschätzt. */
export function usePlatform(): Platform {
  const [platform, setPlatform] = useState<Platform>(detectFallbackPlatform);
  useEffect(() => {
    let cancelled = false;
    invoke<string>("get_platform")
      .then((p) => {
        if (cancelled) return;
        if (p === "macos" || p === "windows" || p === "linux" || p === "android") {
          setPlatform(p);
        }
      })
      .catch((err) => {
        console.warn("Konnte Plattform nicht über Tauri-Command ermitteln, nutze Fallback:", err);
      });
    return () => {
      cancelled = true;
    };
  }, []);
  return platform;
}

/** True on macOS-like systems (Command key, Finder). Reads `navigator.platform`. */
export function isMacLike(): boolean {
  return typeof navigator !== "undefined" && navigator.platform.toLowerCase().includes("mac");
}

/** True on Windows-like systems (Explorer). Reads `navigator.platform`. */
export function isWindowsLike(): boolean {
  return typeof navigator !== "undefined" && navigator.platform.toLowerCase().includes("win");
}
