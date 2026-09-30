import type { IconType } from "react-icons";
import { MdCloud } from "react-icons/md";
import { RiGrokAiFill, RiOpenaiFill, RiZhipuAiFill } from "react-icons/ri";
import { SiAnthropic, SiDeepseek, SiGooglegemini, SiOllama, SiXiaomi } from "react-icons/si";
import type { AIProviderKind } from "@/types/global";

const PROVIDER_ICONS: Partial<Record<AIProviderKind, IconType>> = {
  openai: RiOpenaiFill,
  anthropic: SiAnthropic,
  gemini: SiGooglegemini,
  deepseek: SiDeepseek,
  xai: RiGrokAiFill,
  zai: RiZhipuAiFill,
  ollama: SiOllama,
  mimo: SiXiaomi,
};

const PROVIDER_ICON_INITIALS: Partial<Record<AIProviderKind, string>> = {
  cohere: "C",
};

export function ProviderBadge({
  kind,
  iconDataUrl,
  size = "default",
}: {
  kind?: AIProviderKind | null;
  iconDataUrl?: string | null;
  size?: "default" | "sm";
}) {
  const Icon = kind ? PROVIDER_ICONS[kind] : undefined;
  const initials = kind ? PROVIDER_ICON_INITIALS[kind] : undefined;
  const isSmall = size === "sm";
  return (
    <span
      aria-hidden="true"
      className={`grid shrink-0 place-items-center rounded-full bg-primary/10 text-primary ${isSmall ? "size-5" : "size-6"}`}
    >
      {iconDataUrl?.startsWith("data:image/png;base64,") ? (
        <img src={iconDataUrl} alt="" className="size-full rounded-full object-cover" />
      ) : Icon ? (
        <Icon className={isSmall ? "size-3.5 text-primary" : "size-4 text-primary"} />
      ) : initials ? (
        <span className={`font-bold leading-none ${isSmall ? "text-[0.5rem]" : "text-[0.625rem]"}`}>
          {initials}
        </span>
      ) : (
        <MdCloud className={isSmall ? "size-3.5 text-primary" : "size-4 text-primary"} />
      )}
    </span>
  );
}
