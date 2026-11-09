import { useEffect, useRef } from "react";
import { EditorContent, useEditor } from "@tiptap/react";
import StarterKit from "@tiptap/starter-kit";
import Placeholder from "@tiptap/extension-placeholder";
import { Markdown } from "@tiptap/markdown";
import Image from "@tiptap/extension-image";
import { resolveImageSrc } from "@/lib/attachments";
import { cn } from "@/lib/utils";

/**
 * TipTap editor that reads and writes Markdown. `onChange` fires on every edit with the
 * current Markdown; callers debounce persistence. Escape blurs (returning hotkeys to the player).
 */
export function MarkdownEditor({
  value,
  onChange,
  placeholder,
  autoFocus,
  className,
  onBlur,
  ariaLabel,
}: {
  value: string;
  onChange: (md: string) => void;
  placeholder?: string;
  autoFocus?: boolean;
  className?: string;
  onBlur?: () => void;
  ariaLabel?: string;
}) {
  const onChangeRef = useRef(onChange);
  onChangeRef.current = onChange;
  const onBlurRef = useRef(onBlur);
  onBlurRef.current = onBlur;
  const lastEmitted = useRef(value);

  const editor = useEditor({
    extensions: [
      StarterKit.configure({ heading: { levels: [1, 2, 3] }, link: { openOnClick: false } }),
      Placeholder.configure({ placeholder: placeholder ?? "Write in Markdown…" }),
      // Frames are stored as `lectern-attachment:` links; only the rendered <img> src is resolved,
      // so the Markdown keeps the portable link.
      Image.extend({
        renderHTML({ HTMLAttributes }) {
          return [
            "img",
            {
              ...HTMLAttributes,
              src: resolveImageSrc(HTMLAttributes.src as string),
              class: "my-2 max-w-full rounded-md border",
            },
          ];
        },
      }),
      Markdown,
    ],
    content: value,
    contentType: "markdown",
    autofocus: autoFocus ? "end" : false,
    editorProps: {
      attributes: { class: "min-h-16 focus:outline-none", ...(ariaLabel ? { "aria-label": ariaLabel } : {}) },
      handleKeyDown: (view, e) => {
        if (e.key === "Escape") {
          (view.dom as HTMLElement).blur();
          return true;
        }
        return false;
      },
    },
    onUpdate: ({ editor }) => {
      const md = editor.getMarkdown();
      lastEmitted.current = md;
      onChangeRef.current(md);
    },
    onBlur: () => onBlurRef.current?.(),
  });

  // External value changes (e.g. switching lessons) replace the content.
  useEffect(() => {
    if (editor && value !== lastEmitted.current) {
      lastEmitted.current = value;
      editor.commands.setContent(value, { contentType: "markdown", emitUpdate: false });
    }
  }, [editor, value]);

  return <EditorContent editor={editor} className={cn("prose-lectern", className)} />;
}
