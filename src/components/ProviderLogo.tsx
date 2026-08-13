import codexDark from "../assets/providers/codex-dark.png";
import codexLight from "../assets/providers/codex-light.png";
import claudeLogo from "../assets/providers/claude.svg";

type ProviderLogoProps = {
  providerName: string;
  className?: string;
  fallback?: string;
};

function providerInitials(providerName: string): string {
  return providerName
    .split(/\s+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((part) => part[0])
    .join("")
    .toUpperCase();
}

export function ProviderLogo({
  providerName,
  className = "",
  fallback,
}: ProviderLogoProps) {
  const isCodex = providerName.trim().toLowerCase() === "codex";
  const isClaude = providerName.trim().toLowerCase() === "claude code";

  return (
    <span
      className={`${className} provider-logo ${isCodex ? "codex" : isClaude ? "claude" : ""}`.trim()}
      aria-hidden="true"
    >
      {isCodex ? (
        <>
          <img
            className="provider-logo-image dark"
            src={codexDark}
            alt=""
          />
          <img
            className="provider-logo-image light"
            src={codexLight}
            alt=""
          />
        </>
      ) : isClaude ? (
        <img className="provider-logo-image" src={claudeLogo} alt="" />
      ) : (
        fallback ?? providerInitials(providerName)
      )}
    </span>
  );
}
