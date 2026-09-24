import type { ComponentProps } from "react";
import { buttonVariants, type ButtonProps } from "@fabrials/ui";

export function LinkButton({ variant = "outline", size = "sm", className, ...props }: ComponentProps<"a"> & Pick<ButtonProps, "variant" | "size">) {
  return <a className={buttonVariants({ variant, size, className })} {...props} />;
}
