export interface ClipInfo {
  id: string;
  path: string;
  file_name: string;
  size_bytes: number;
}

export interface FrameMetrics {
  timestamp_secs: number;
  sharpness: number;
  mean_luminance: number;
  motion_incoherence: number;
  face_detected: boolean;
}

export type Classification = "BestTake" | "UsableBRoll" | "DiscardTake";

export interface AnalyzedClip {
  clip: ClipInfo;
  frames: FrameMetrics[];
  min_sharpness: number;
  max_motion_incoherence: number;
  min_luminance: number;
  /** Informational only - doesn't affect score/classification. */
  contains_face: boolean;
  score: number;
  classification: Classification;
  flags: string[];
}

export interface ProjectSummary {
  total_clips: number;
  best_take_count: number;
  usable_broll_count: number;
  discard_count: number;
  blurry_flagged: number;
  shaky_flagged: number;
  blackout_flagged: number;
}

export interface AnalysisResult {
  clips: AnalyzedClip[];
  summary: ProjectSummary;
  errors: string[];
}

export type Tolerance = "aggressive" | "conservative";

export type LicenseTier = "founding" | "pro-freelancer" | "studio-suite";

export type LicenseStatus =
  | { state: "not_activated"; machine_fingerprint: string }
  | {
      state: "active";
      machine_fingerprint: string;
      license_id: string;
      customer_email: string;
      tier: LicenseTier;
    }
  | { state: "invalid"; machine_fingerprint: string; reason: string };
