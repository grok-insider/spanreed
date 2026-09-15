export const FABRIAL_MARK_DARK = "/brand/logo-dark.png";
export const FABRIAL_MARK_LIGHT = "/brand/logo-light.png";

export function FabrialBrandMark() {
  return <span className="fb-brand-symbol">
    <img className="fb-brand-mark fb-brand-mark-dark" src={FABRIAL_MARK_DARK} alt="" />
    <img className="fb-brand-mark fb-brand-mark-light" src={FABRIAL_MARK_LIGHT} alt="" />
  </span>;
}
