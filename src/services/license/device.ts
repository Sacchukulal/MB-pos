import { invoke } from "@tauri-apps/api/core";

export interface DeviceInfo {
  id: string;
  name: string;
}

let cached: DeviceInfo | null = null;

/**
 * This machine's stable hardware-bound identifier (SMBIOS UUID with MAC fallback,
 * resolved by the Rust side) plus a friendly computer name. Used to lock a license
 * to a single desktop. Cached for the session.
 */
export async function getDeviceInfo(): Promise<DeviceInfo> {
  if (cached) return cached;
  try {
    const info = await invoke<DeviceInfo>("get_device_id");
    cached = {
      id: info?.id || "UNKNOWN-DEVICE",
      name: info?.name || "Unknown PC",
    };
  } catch (e) {
    console.error("Failed to read device id:", e);
    cached = { id: "UNKNOWN-DEVICE", name: "Unknown PC" };
  }
  return cached;
}
