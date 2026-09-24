import { Accordion as BaseAccordion } from "@base-ui/react/accordion";
import { type StyledProps } from "./shared";
export declare function Accordion({ className, ...props }: StyledProps<BaseAccordion.Root.Props>): import("react").JSX.Element;
export declare function AccordionItem({ className, ...props }: StyledProps<BaseAccordion.Item.Props>): import("react").JSX.Element;
export declare function AccordionTrigger({ className, children, ...props }: StyledProps<BaseAccordion.Trigger.Props>): import("react").JSX.Element;
export declare function AccordionContent({ className, children, ...props }: StyledProps<BaseAccordion.Panel.Props>): import("react").JSX.Element;
