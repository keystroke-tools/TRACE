import { useId, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";

type TooltipPosition = { left: number; top: number };

export function Tooltip({ children, content, className = "" }: { children: ReactNode; content: ReactNode; className?: string }) {
  const id = useId();
  const anchor = useRef<HTMLSpanElement>(null);
  const bubble = useRef<HTMLSpanElement>(null);
  const [visible, setVisible] = useState(false);
  const [position, setPosition] = useState<TooltipPosition | null>(null);

  useLayoutEffect(() => {
    if (!visible || !anchor.current || !bubble.current) return;
    const anchorRect = anchor.current.getBoundingClientRect();
    const bubbleRect = bubble.current.getBoundingClientRect();
    const gap = 8;
    const edge = 10;
    const left = Math.min(
      window.innerWidth - bubbleRect.width - edge,
      Math.max(edge, anchorRect.left + anchorRect.width / 2 - bubbleRect.width / 2),
    );
    const above = anchorRect.top - bubbleRect.height - gap;
    const top = above >= edge ? above : anchorRect.bottom + gap;
    setPosition({ left, top });
  }, [visible, content]);

  if (!content) return <>{children}</>;

  return (
    <span
      ref={anchor}
      className={`inline-flex ${className}`}
      aria-describedby={visible ? id : undefined}
      onMouseEnter={() => setVisible(true)}
      onMouseLeave={() => setVisible(false)}
      onFocusCapture={() => setVisible(true)}
      onBlurCapture={() => setVisible(false)}
    >
      {children}
      {visible && typeof document !== "undefined" && createPortal(
        <span
          ref={bubble}
          id={id}
          role="tooltip"
          style={position ?? { left: -10_000, top: -10_000 }}
          className="pointer-events-none fixed z-[100] max-w-72 border border-trace-divider bg-trace-black px-2.5 py-2 font-sans text-[11px] leading-4 tracking-normal text-trace-soft shadow-[0_8px_24px_#000]"
        >
          {content}
        </span>,
        document.body,
      )}
    </span>
  );
}
