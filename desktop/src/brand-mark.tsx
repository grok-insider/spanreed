export const FABRIAL_MARK_DARK = "/brand/logo-dark.png";
export const FABRIAL_MARK_LIGHT = "/brand/logo-light.png";

export function FabrialBrandMark() {
  return <span className="sr-brand-mark" aria-hidden="true">
    <img className="sr-brand-mark-dark" src={FABRIAL_MARK_DARK} alt="" />
    <img className="sr-brand-mark-light" src={FABRIAL_MARK_LIGHT} alt="" />
  </span>;
}
