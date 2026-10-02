import { Accordion as BaseAccordion } from "@base-ui/react/accordion";
import { type ReactNode, type Ref } from "react";
import { type StyledProps } from "./shared";
type AccordionVariant = "default" | "rows";
/**
 * `variant="rows"` is a stack of independent tools: each row is a 44 px header (an icon, the title, tags and a chevron) over
 * a body that stays mounted while it is closed, so a form inside keeps what was typed and any controller it owns. The open row
 * is marked like the current item of a list (accent fill and a 2 px Stormlight bar). Give the rows `multiple` to let several
 * stay open, and set `--fui-accordion-pad` to change the inline padding of the rows and their bodies.
 */
export declare function Accordion({ className, variant, keepMounted, ...props }: StyledProps<BaseAccordion.Root.Props> & {
    variant?: AccordionVariant;
}): import("react").JSX.Element;
export declare function AccordionItem({ className, ...props }: StyledProps<BaseAccordion.Item.Props>): import("react").JSX.Element;
export type AccordionTriggerProps = StyledProps<BaseAccordion.Trigger.Props> & {
    /** The level of the heading that wraps the trigger (2 to 4). Default 3, Base UI's. */
    headingLevel?: 2 | 3 | 4;
    /** A ref to the heading element. The heading then takes `tabIndex={-1}` and hands focus to the trigger, so a command or a
     * deep link can focus "the tool" and land on the control a person can act on. */
    headingRef?: Ref<HTMLHeadingElement>;
    /** Decorative icon before the title (`variant="rows"`). */
    icon?: ReactNode;
    /**
     * Tags, counts or a state beside the title, OUTSIDE the trigger: they are not part of its name and do not make the row a
     * bigger target. The trigger is described by them (`aria-describedby`). Non-interactive: put nothing to click in it.
     */
    aside?: ReactNode;
};
export declare function AccordionTrigger({ className, children, headingLevel, headingRef, icon, aside, ...props }: AccordionTriggerProps): import("react").JSX.Element;
export declare function AccordionContent({ className, children, ...props }: StyledProps<BaseAccordion.Panel.Props>): import("react").JSX.Element;
export {};
