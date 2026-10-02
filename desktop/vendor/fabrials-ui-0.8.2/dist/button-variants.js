import { classes } from "./shared.js";
function buttonVariants({ variant = "default", size = "default", className } = {}) {
  return classes("fui-button", `fui-button-${variant}`, `fui-button-size-${size}`, className);
}
export {
  buttonVariants
};
