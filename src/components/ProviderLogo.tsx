import codexDark from "../assets/providers/codex-dark.png";
import codexLight from "../assets/providers/codex-light.png";

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

  return (
    <span
      className={`${className} provider-logo ${isCodex ? "codex" : ""}`.trim()}
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
      ) : (
        fallback ?? providerInitials(providerName)
      )}
    </span>
  );
}
