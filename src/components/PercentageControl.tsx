import type { CSSProperties } from "react";

type PercentageControlProps = {
  label: string;
  value: string;
  onChange: (value: string) => void;
  min: number;
  max: number;
  step: number;
  sliderMin?: number;
  sliderMax?: number;
  sliderStep?: number;
  helperText?: string;
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
  sliderMin = min,
  sliderMax = max,
  sliderStep = step,
  helperText,
  allowEmpty = false,
  autoFocus = false,
}: PercentageControlProps) {
  const numericValue = Number(value);
  const hasValue = value.trim() !== "" && Number.isFinite(numericValue);
  const boundedMax = Math.max(min, max);
  const boundedSliderMax = Math.max(sliderMin, sliderMax);
  const sliderValue = hasValue
    ? Math.min(boundedMax, Math.max(min, numericValue))
    : min;
  const progress =
    boundedSliderMax === sliderMin
      ? 100
      : ((sliderValue - sliderMin) / (boundedSliderMax - sliderMin)) * 100;
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
          min={sliderMin}
          max={boundedSliderMax}
          step={sliderStep}
          value={sliderValue}
          style={rangeStyle}
          onChange={(event) => {
            const nextValue = Math.min(
              boundedMax,
              Math.max(min, Number(event.currentTarget.value)),
            );
            onChange(String(nextValue));
          }}
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
          ? helperText ??
            `${formatPercent(sliderValue)}% selected · allowed range ${formatPercent(min)}–${formatPercent(max)}%`
          : "Disabled · drag the slider or enter a value to enable"}
      </small>
    </label>
  );
}

function formatPercent(value: number): string {
  return Number.isInteger(value)
    ? String(value)
    : value.toFixed(2).replace(/0+$/, "").replace(/\.$/, "");
}
