import * as React from "react";
import { Slot } from "@radix-ui/react-slot";
import { cva, type VariantProps } from "class-variance-authority";

import { cn } from "@/lib/utils";

const buttonVariants = cva(
  "inline-flex h-8 items-center justify-center gap-1.5 whitespace-nowrap rounded-md border border-transparent px-3 text-[13px] font-medium transition-all outline-none disabled:pointer-events-none disabled:opacity-50 [&_svg]:pointer-events-none [&_svg]:size-4 [&_svg]:shrink-0",
  {
    variants: {
      variant: {
        default:
          "border-primary bg-primary text-primary-foreground shadow-xs hover:bg-primary/90",
        destructive:
          "border-destructive bg-destructive text-destructive-foreground shadow-xs hover:bg-destructive/90",
        outline:
          "border-border bg-background text-foreground shadow-none hover:border-border-strong hover:bg-muted",
        secondary:
          "border-border bg-card text-foreground shadow-none hover:border-border-strong hover:bg-muted",
        ghost:
          "border-transparent bg-transparent text-muted-foreground shadow-none hover:bg-muted hover:text-foreground",
        link: "h-auto border-0 bg-transparent px-0 text-primary underline-offset-4 hover:underline",
      },
      size: {
        default: "h-8 px-3",
        sm: "h-7 rounded-md px-2.5 text-xs",
        lg: "h-9 rounded-md px-4",
        icon: "size-8 px-0",
      },
    },
    defaultVariants: {
      variant: "default",
      size: "default",
    },
  },
);

function Button({
  className,
  variant,
  size,
  asChild = false,
  ...props
}: React.ComponentProps<"button"> &
  VariantProps<typeof buttonVariants> & {
    asChild?: boolean;
  }) {
  const Comp = asChild ? Slot : "button";

  return (
    <Comp
      data-slot="button"
      className={cn(buttonVariants({ variant, size, className }))}
      {...props}
    />
  );
}

export { Button, buttonVariants };
