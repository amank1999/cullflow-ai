import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import type { AnalysisResult, ClipInfo, LicenseStatus, Tolerance } from "./types";

export async function pickFolder(): Promise<string | null> {
  const selected = await open({ directory: true, multiple: false });
  if (Array.isArray(selected)) return selected[0] ?? null;
  return selected;
}

export async function scanFolder(path: string): Promise<ClipInfo[]> {
  return invoke<ClipInfo[]>("scan_folder", { path });
}

export async function analyzeProject(tolerance: Tolerance): Promise<AnalysisResult> {
  return invoke<AnalysisResult>("analyze_project", { tolerance });
}

export async function setTolerance(tolerance: Tolerance): Promise<AnalysisResult> {
  return invoke<AnalysisResult>("set_tolerance", { tolerance });
}

export async function exportXml(): Promise<string | null> {
  const outputPath = await save({
    defaultPath: "CullFlow-Export.fcpxml",
    filters: [{ name: "FCPXML", extensions: ["fcpxml", "xml"] }],
  });
  if (!outputPath) return null;
  return invoke<string>("export_xml", { outputPath });
}

export async function getLicenseStatus(): Promise<LicenseStatus> {
  return invoke<LicenseStatus>("get_license_status");
}

export async function activateLicense(key: string): Promise<LicenseStatus> {
  return invoke<LicenseStatus>("activate_license", { key });
}
