import {
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent,
  type PointerEvent,
} from "react";

type ThresholdKey = "warn" | "confirm" | "stop";

type PolicyThresholdControlProps = {
  warnAt: string;
  confirmAt: string;
  stopAt: string;
  onWarnChange: (value: string) => void;
  onConfirmChange: (value: string) => void;
  onStopChange: (value: string) => void;
};

export function PolicyThresholdControl({
  warnAt,
  confirmAt,
  stopAt,
  onWarnChange,
  onConfirmChange,
  onStopChange,
}: PolicyThresholdControlProps) {
  const trackRef = useRef<HTMLDivElement>(null);
  const [active, setActive] = useState<ThresholdKey | null>(null);
  const values = {
    warn: optionalPercent(warnAt),
    confirm: optionalPercent(confirmAt),
    stop: optionalPercent(stopAt),
  };
  const setters = {
    warn: onWarnChange,
    confirm: onConfirmChange,
    stop: onStopChange,
  };

  function bounds(key: ThresholdKey): [number, number] {
    if (key === "warn") {
      return [0.01, values.confirm ?? values.stop ?? 100];
    }
    if (key === "confirm") {
      return [values.warn ?? 0.01, values.stop ?? 100];
    }
    return [values.confirm ?? values.warn ?? 0.01, 100];
  }

  function setThreshold(key: ThresholdKey, next: number) {
    const [min, max] = bounds(key);
    setters[key](String(Math.min(max, Math.max(min, next))));
  }

  function valueFromPointer(clientX: number): number {
    const bounds = trackRef.current?.getBoundingClientRect();
    if (!bounds || bounds.width === 0) {
      return 0;
    }
    return Math.round(
      Math.min(100, Math.max(0, ((clientX - bounds.left) / bounds.width) * 100)),
    );
  }

  function nearestThreshold(next: number): ThresholdKey {
    const enabled = (Object.keys(values) as ThresholdKey[]).filter(
      (key) => values[key] !== null,
    );
    if (enabled.length === 0) {
      return "warn";
    }
    return enabled.reduce((nearest, key) =>
      Math.abs((values[key] ?? next) - next) <
      Math.abs((values[nearest] ?? next) - next)
        ? key
        : nearest,
    );
  }

  function startTrackDrag(event: PointerEvent<HTMLDivElement>) {
    const next = valueFromPointer(event.clientX);
    const key = nearestThreshold(next);
    setActive(key);
    setThreshold(key, next);
    event.currentTarget.setPointerCapture(event.pointerId);
  }

  function startHandleDrag(
    event: PointerEvent<HTMLButtonElement>,
    key: ThresholdKey,
  ) {
    event.stopPropagation();
    setActive(key);
    event.currentTarget.setPointerCapture(event.pointerId);
  }

  function continueDrag(event: PointerEvent<HTMLElement>) {
    if (active) {
      setThreshold(active, valueFromPointer(event.clientX));
    }
  }

  function handleKeyDown(
    event: KeyboardEvent<HTMLButtonElement>,
    key: ThresholdKey,
  ) {
    const current = values[key];
    if (current === null) {
      return;
    }
    const direction =
      event.key === "ArrowRight" || event.key === "ArrowUp"
        ? 1
        : event.key === "ArrowLeft" || event.key === "ArrowDown"
          ? -1
          : 0;
    if (direction !== 0) {
      event.preventDefault();
      setThreshold(key, current + direction);
    }
  }

  const trackStyle = {
    "--warn-at": `${values.warn ?? 0}%`,
    "--confirm-at": `${values.confirm ?? values.warn ?? 0}%`,
    "--stop-at": `${values.stop ?? 100}%`,
  } as CSSProperties;

  return (
    <div className="policy-threshold-control">
      <div
        ref={trackRef}
        className="policy-threshold-track"
        style={trackStyle}
        onPointerDown={startTrackDrag}
        onPointerMove={continueDrag}
        onPointerUp={() => setActive(null)}
        onPointerCancel={() => setActive(null)}
      >
        {(Object.keys(values) as ThresholdKey[]).map((key) => {
          const value = values[key];
          if (value === null) {
            return null;
          }
          const [min, max] = bounds(key);
          return (
            <button
              key={key}
              type="button"
              className={`policy-threshold-handle ${key} ${active === key ? "active" : ""}`}
              style={{ left: `${value}%` }}
              onPointerDown={(event) => startHandleDrag(event, key)}
              onPointerMove={continueDrag}
              onPointerUp={() => setActive(null)}
              onPointerCancel={() => setActive(null)}
              onKeyDown={(event) => handleKeyDown(event, key)}
              role="slider"
              aria-label={`${title(key)} threshold`}
              aria-valuemin={min}
              aria-valuemax={max}
              aria-valuenow={value}
              title={`${title(key)} at ${value}%`}
            >
              <span aria-hidden="true">{shortTitle(key)}</span>
            </button>
          );
        })}
      </div>
      <div className="policy-threshold-scale" aria-hidden="true">
        <span>0%</span>
        <span>100%</span>
      </div>
      <div className="policy-threshold-inputs">
        <ThresholdInput
          label="Warn"
          value={warnAt}
          min={0.01}
          max={values.confirm ?? values.stop ?? 100}
          onChange={onWarnChange}
        />
        <ThresholdInput
          label="Confirm"
          value={confirmAt}
          min={values.warn ?? 0.01}
          max={values.stop ?? 100}
          onChange={onConfirmChange}
        />
        <ThresholdInput
          label="Stop"
          value={stopAt}
          min={values.confirm ?? values.warn ?? 0.01}
          max={100}
          onChange={onStopChange}
        />
      </div>
      <p className="policy-threshold-help">
        Drag a marker in whole percentages, or type an exact value. Thresholds
        stay ordered: Warn ≤ Confirm ≤ Stop. Clear a value to disable it.
      </p>
    </div>
  );
}

type ThresholdInputProps = {
  label: string;
  value: string;
  min: number;
  max: number;
  onChange: (value: string) => void;
};

function ThresholdInput({
  label,
  value,
  min,
  max,
  onChange,
}: ThresholdInputProps) {
  return (
    <label className="field">
      <span>{label}</span>
      <div className="input-with-suffix percentage-number">
        <input
          type="number"
          min={min}
          max={max}
          step="0.01"
          value={value}
          onChange={(event) => onChange(event.currentTarget.value)}
        />
        <span>%</span>
      </div>
    </label>
  );
}

function optionalPercent(value: string): number | null {
  const parsed = Number(value);
  return value.trim() === "" || !Number.isFinite(parsed)
    ? null
    : Math.min(100, Math.max(0.01, parsed));
}

function title(key: ThresholdKey): string {
  return key.charAt(0).toUpperCase() + key.slice(1);
}

function shortTitle(key: ThresholdKey): string {
  return key.charAt(0).toUpperCase();
}
