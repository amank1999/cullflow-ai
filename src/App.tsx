import { useEffect, useMemo, useState } from "react";
import {
  activateLicense,
  analyzeProject,
  exportXml,
  getLicenseStatus,
  pickFolder,
  scanFolder,
  setTolerance,
} from "./api";
import type { AnalysisResult, ClipInfo, Classification, LicenseStatus, Tolerance } from "./types";
import "./App.css";

const TIER_LABEL: Record<string, string> = {
  founding: "Founding Pass",
  "pro-freelancer": "Pro Freelancer",
  "studio-suite": "Studio Suite",
};

type Stage = "idle" | "scanned" | "analyzing" | "analyzed" | "exporting";

const CLASSIFICATION_LABEL: Record<Classification, string> = {
  BestTake: "Best Take",
  UsableBRoll: "Usable B-Roll",
  DiscardTake: "Discard",
};

const CLASSIFICATION_CLASS: Record<Classification, string> = {
  BestTake: "badge badge--green",
  UsableBRoll: "badge badge--cyan",
  DiscardTake: "badge badge--red",
};

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let value = bytes;
  let unitIndex = -1;
  do {
    value /= 1024;
    unitIndex++;
  } while (value >= 1024 && unitIndex < units.length - 1);
  return `${value.toFixed(1)} ${units[unitIndex]}`;
}

export default function App() {
  const [folder, setFolder] = useState<string | null>(null);
  const [clips, setClips] = useState<ClipInfo[]>([]);
  const [tolerance, setToleranceState] = useState<Tolerance>("conservative");
  const [result, setResult] = useState<AnalysisResult | null>(null);
  const [stage, setStage] = useState<Stage>("idle");
  const [error, setError] = useState<string | null>(null);
  const [exportedPath, setExportedPath] = useState<string | null>(null);
  const [license, setLicense] = useState<LicenseStatus | null>(null);

  useEffect(() => {
    getLicenseStatus().then(setLicense).catch(() => {});
  }, []);

  const totalSize = useMemo(
    () => clips.reduce((sum, c) => sum + c.size_bytes, 0),
    [clips],
  );

  async function handlePickFolder() {
    setError(null);
    setExportedPath(null);
    try {
      const path = await pickFolder();
      if (!path) return;
      const found = await scanFolder(path);
      setFolder(path);
      setClips(found);
      setResult(null);
      setStage("scanned");
    } catch (e) {
      setError(String(e));
    }
  }

  async function handleScanProject() {
    setStage("analyzing");
    setError(null);
    try {
      const analysis = await analyzeProject(tolerance);
      setResult(analysis);
      setStage("analyzed");
    } catch (e) {
      setError(String(e));
      setStage("scanned");
    }
  }

  async function handleToleranceChange(next: Tolerance) {
    setToleranceState(next);
    if (stage !== "analyzed") return;
    try {
      const analysis = await setTolerance(next);
      setResult(analysis);
    } catch (e) {
      setError(String(e));
    }
  }

  async function handleExport() {
    setStage("exporting");
    setError(null);
    try {
      const path = await exportXml();
      setExportedPath(path);
    } catch (e) {
      setError(String(e));
      getLicenseStatus().then(setLicense).catch(() => {});
    } finally {
      setStage("analyzed");
    }
  }

  async function handleActivate(key: string) {
    setError(null);
    try {
      const status = await activateLicense(key);
      setLicense(status);
    } catch (e) {
      setError(String(e));
    }
  }

  return (
    <div className="app">
      <header className="app__header">
        <div>
          <h1>CullFlow AI</h1>
          <p className="app__tagline">
            Offline footage culling &amp; NLE timeline generation. 0 bytes leave this machine.
          </p>
        </div>
        <LicensePanel license={license} onActivate={handleActivate} />
      </header>

      <ol className="steps">
        <li className={stage === "idle" ? "steps__item steps__item--active" : "steps__item"}>
          <span className="steps__index">1</span>
          <div>
            <h2>Folder Ingest</h2>
            <p>Point CullFlow at a raw footage folder (500GB+ is fine — it never leaves disk).</p>
            <button onClick={handlePickFolder}>Choose Folder&hellip;</button>
            {folder && (
              <p className="steps__meta">
                {clips.length} clip{clips.length === 1 ? "" : "s"} found in <code>{folder}</code>
                {clips.length > 0 && ` · ${formatBytes(totalSize)}`}
              </p>
            )}
          </div>
        </li>

        <li className={stage === "scanned" || stage === "analyzing" ? "steps__item steps__item--active" : "steps__item"}>
          <span className="steps__index">2</span>
          <div>
            <h2>Proxy &amp; Analysis</h2>
            <p>Extracts a lightweight proxy per clip and scores sharpness, jitter, and blackout.</p>
            <div className="tolerance-select">
              <label>
                <input
                  type="radio"
                  name="tolerance"
                  checked={tolerance === "conservative"}
                  onChange={() => handleToleranceChange("conservative")}
                />
                Conservative
              </label>
              <label>
                <input
                  type="radio"
                  name="tolerance"
                  checked={tolerance === "aggressive"}
                  onChange={() => handleToleranceChange("aggressive")}
                />
                Aggressive
              </label>
            </div>
            <button
              onClick={handleScanProject}
              disabled={clips.length === 0 || stage === "analyzing"}
            >
              {stage === "analyzing" ? "Scanning…" : "Scan Project"}
            </button>
          </div>
        </li>

        <li className={stage === "analyzed" || stage === "exporting" ? "steps__item steps__item--active" : "steps__item"}>
          <span className="steps__index">3</span>
          <div>
            <h2>Visual Audit</h2>
            {result ? (
              <>
                <div className="summary-cards">
                  <SummaryCard label="Best Takes" value={result.summary.best_take_count} tone="green" />
                  <SummaryCard label="Usable B-Roll" value={result.summary.usable_broll_count} tone="cyan" />
                  <SummaryCard label="Discard" value={result.summary.discard_count} tone="red" />
                  <SummaryCard label="Blurry Flagged" value={result.summary.blurry_flagged} tone="neutral" />
                  <SummaryCard label="Shaky Flagged" value={result.summary.shaky_flagged} tone="neutral" />
                  <SummaryCard label="Blackout Flagged" value={result.summary.blackout_flagged} tone="neutral" />
                </div>
                {result.errors.length > 0 && (
                  <details className="errors">
                    <summary>{result.errors.length} clip(s) failed to analyze</summary>
                    <ul>
                      {result.errors.map((e) => (
                        <li key={e}>{e}</li>
                      ))}
                    </ul>
                  </details>
                )}
                <table className="clip-table">
                  <thead>
                    <tr>
                      <th>Clip</th>
                      <th>Score</th>
                      <th>Classification</th>
                      <th>Flags</th>
                    </tr>
                  </thead>
                  <tbody>
                    {result.clips.map((c) => (
                      <tr key={c.clip.id}>
                        <td>{c.clip.file_name}</td>
                        <td>{c.score.toFixed(0)}</td>
                        <td>
                          <span className={CLASSIFICATION_CLASS[c.classification]}>
                            {CLASSIFICATION_LABEL[c.classification]}
                          </span>
                        </td>
                        <td>{c.flags.join(", ") || "—"}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </>
            ) : (
              <p className="steps__meta">Run a scan to see results here.</p>
            )}
          </div>
        </li>

        <li className={stage === "exporting" ? "steps__item steps__item--active" : "steps__item"}>
          <span className="steps__index">4</span>
          <div>
            <h2>One-Click XML Export</h2>
            <p>Generates an FCPXML sequence (imports into DaVinci Resolve &amp; Premiere Pro) with color-coded markers, mapped back to your original files.</p>
            {license?.state !== "active" && (
              <p className="steps__meta steps__meta--warn">
                Requires an activated license - scanning and scoring above stay free to try.
              </p>
            )}
            <button onClick={handleExport} disabled={!result || stage === "exporting"}>
              {stage === "exporting" ? "Exporting…" : "Generate NLE Sequence"}
            </button>
            {exportedPath && <p className="steps__meta">Exported to <code>{exportedPath}</code></p>}
          </div>
        </li>
      </ol>

      {error && <div className="error-banner">{error}</div>}
    </div>
  );
}

function SummaryCard({
  label,
  value,
  tone,
}: {
  label: string;
  value: number;
  tone: "green" | "cyan" | "red" | "neutral";
}) {
  return (
    <div className={`summary-card summary-card--${tone}`}>
      <div className="summary-card__value">{value}</div>
      <div className="summary-card__label">{label}</div>
    </div>
  );
}

function LicensePanel({
  license,
  onActivate,
}: {
  license: LicenseStatus | null;
  onActivate: (key: string) => void;
}) {
  const [keyInput, setKeyInput] = useState("");

  if (!license) return null;

  if (license.state === "active") {
    return (
      <div className="license-panel license-panel--active">
        <span className="badge badge--green">Licensed</span>
        <span className="license-panel__detail">
          {TIER_LABEL[license.tier] ?? license.tier} · {license.customer_email}
        </span>
      </div>
    );
  }

  return (
    <div className="license-panel">
      {license.state === "invalid" && (
        <p className="license-panel__reason">License invalid: {license.reason}</p>
      )}
      <form
        className="license-panel__form"
        onSubmit={(e) => {
          e.preventDefault();
          if (keyInput.trim()) onActivate(keyInput.trim());
        }}
      >
        <input
          type="text"
          placeholder="Paste your license key…"
          value={keyInput}
          onChange={(e) => setKeyInput(e.target.value)}
        />
        <button type="submit">Activate</button>
      </form>
      <p className="license-panel__fingerprint">
        Machine ID: <code>{license.machine_fingerprint}</code>
      </p>
    </div>
  );
}
