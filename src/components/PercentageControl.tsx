import type { CSSProperties } from "react";

type PercentageControlProps = {
  label: string;
  value: string;
  onChange: (value: string) => void;
  min: number;
  max: number;
  step: number;
  allowEmpty?: boolean;
  autoFocus?: boolean;
};

export function PercentageControl({
  label,
  value,
  onChange,
  min,
  max,
  step,
  allowEmpty = false,
  autoFocus = false,
}: PercentageControlProps) {
  const numericValue = Number(value);
  const hasValue = value.trim() !== "" && Number.isFinite(numericValue);
  const boundedMax = Math.max(min, max);
  const sliderValue = hasValue
    ? Math.min(boundedMax, Math.max(min, numericValue))
    : min;
  const progress =
    boundedMax === min ? 100 : ((sliderValue - min) / (boundedMax - min)) * 100;
  const rangeStyle = {
    "--range-progress": `${progress}%`,
  } as CSSProperties;

  return (
    <label className="field percentage-control">
      <span>{label}</span>
      <div className="percentage-control-row">
        <input
          className={`percent-range ${hasValue ? "" : "inactive"}`}
          type="range"
          min={min}
          max={boundedMax}
          step={step}
          value={sliderValue}
          style={rangeStyle}
          onChange={(event) => onChange(event.currentTarget.value)}
          aria-label={`${label} slider`}
        />
        <div className="input-with-suffix percentage-number">
          <input
            autoFocus={autoFocus}
            type="number"
            min={min}
            max={max}
            step={step}
            value={value}
            onChange={(event) => onChange(event.currentTarget.value)}
            required={!allowEmpty}
            aria-label={`${label} value`}
          />
          <span>%</span>
        </div>
      </div>
      <small>
        {hasValue
          ? `${formatPercent(sliderValue)}% selected · maximum ${formatPercent(max)}%`
          : "Disabled · drag the slider or enter a value to enable"}
      </small>
    </label>
  );
}

function formatPercent(value: number): string {
  return Number.isInteger(value) ? String(value) : value.toFixed(2).replace(/0+$/, "").replace(/\.$/, "");
}
