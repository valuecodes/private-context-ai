import type { ReactNode } from "react";
import { useEffect, useRef } from "react";

type PopoverProps = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** The anchor button's accessible name. */
  label: string;
  title?: string;
  /** What the panel holds, for `aria-haspopup`. */
  popup: "listbox" | "menu";
  disabled?: boolean;
  anchorClassName: string;
  /** The anchor button's visible content. */
  anchor: ReactNode;
  panelClassName?: string;
  /** The panel's content; `close` closes it and refocuses the anchor. */
  children: (close: () => void) => ReactNode;
};

/**
 * An anchor button and a panel that opens above it, as in the composer.
 * Closed by a click outside or Esc; Esc is marked handled, so it does not
 * also stop a streaming reply.
 */
const Popover = ({
  open,
  onOpenChange,
  label,
  title,
  popup,
  disabled = false,
  anchorClassName,
  anchor,
  panelClassName = "",
  children,
}: PopoverProps) => {
  const root = useRef<HTMLDivElement>(null);
  const button = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    const element = root.current;
    if (!open || element === null) {
      return undefined;
    }
    const onPointerDown = (event: PointerEvent) => {
      if (!(event.target instanceof Node) || !element.contains(event.target)) {
        onOpenChange(false);
      }
    };
    // On the popover itself, so it runs before the chat view's window
    // listener sees the event.
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !event.isComposing) {
        event.preventDefault();
        event.stopPropagation();
        onOpenChange(false);
      }
    };
    document.addEventListener("pointerdown", onPointerDown);
    element.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
      element.removeEventListener("keydown", onKeyDown);
    };
  }, [open, onOpenChange]);

  // Back to the anchor when the panel closes with focus inside it, which
  // then falls to the body; a click elsewhere keeps the focus it moved.
  const wasOpen = useRef(open);
  useEffect(() => {
    const active = document.activeElement;
    if (
      wasOpen.current &&
      !open &&
      (active === null ||
        active === document.body ||
        root.current?.contains(active) === true)
    ) {
      button.current?.focus();
    }
    wasOpen.current = open;
  }, [open]);

  const close = () => {
    onOpenChange(false);
  };

  return (
    <div ref={root} className="relative">
      <button
        ref={button}
        type="button"
        aria-label={label}
        title={title}
        aria-haspopup={popup}
        aria-expanded={open}
        disabled={disabled}
        onClick={() => {
          onOpenChange(!open);
        }}
        className={anchorClassName}
      >
        {anchor}
      </button>
      {open && (
        <div
          className={`absolute bottom-full left-0 z-20 mb-2 rounded-xl border border-zinc-200 bg-white text-sm shadow-lg dark:border-zinc-700 dark:bg-zinc-900 ${panelClassName}`}
        >
          {children(close)}
        </div>
      )}
    </div>
  );
};

export { Popover };
