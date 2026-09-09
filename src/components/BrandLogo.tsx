import blackWordmark from "../../logo/quotafence-logo-black-transparent.png";
import whiteWordmark from "../../logo/quotafence-logo-white-transparent.png";
import blackMark from "../../logo/quotafence-icon-black-transparent.png";
import whiteMark from "../../logo/quotafence-icon-white-transparent.png";

export function BrandLogo({ compact = false }: { compact?: boolean }) {
  return (
    <span className={`quotafence-logo${compact ? " compact" : ""}`} role="img" aria-label="QuotaFence">
      <img className="logo-light logo-wordmark" src={blackWordmark} alt="" />
      <img className="logo-dark logo-wordmark" src={whiteWordmark} alt="" />
      <img className="logo-light logo-symbol" src={blackMark} alt="" />
      <img className="logo-dark logo-symbol" src={whiteMark} alt="" />
    </span>
  );
}
