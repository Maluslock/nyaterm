import { useTranslation } from "react-i18next";
import { MdCheck, MdExpandMore } from "react-icons/md";
import { ProviderBadge } from "@/components/ai/ProviderBadge";
import { Button } from "@/components/ui/button";
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { getModelProviderLabel, getModelReasoningOptions } from "@/lib/aiSettings";
import type { AIModelConfigItem, AIProviderCredential, AIReasoningEffort } from "@/types/global";

function ModelProviderBadge({
  model,
  credentials,
}: {
  model: AIModelConfigItem;
  credentials: AIProviderCredential[];
}) {
  const credential = model.credential_id
    ? credentials.find((item) => item.id === model.credential_id)
    : undefined;
  return (
    <ProviderBadge
      kind={model.backend === "codex" ? "openai" : (credential?.provider_kind ?? model.provider_kind)}
      iconDataUrl={credential?.icon_data_url}
      size="sm"
    />
  );
}

export function ModelCombobox({
  models,
  credentials,
  selectedModel,
  selectedReasoningEffort = "auto",
  open,
  onOpenChange,
  onSelect,
  onSelectReasoningEffort,
  className,
}: {
  models: AIModelConfigItem[];
  credentials: AIProviderCredential[];
  selectedModel: AIModelConfigItem | null;
  selectedReasoningEffort?: AIReasoningEffort;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onSelect: (model: AIModelConfigItem) => void;
  onSelectReasoningEffort: (effort: AIReasoningEffort) => void;
  className?: string;
}) {
  const { t } = useTranslation();
  const reasoningLabel = t(`ai.reasoningEffort.${selectedReasoningEffort}`);
  const reasoningOptions = getModelReasoningOptions(selectedModel);

  return (
    <Popover open={open} onOpenChange={onOpenChange}>
      <PopoverTrigger asChild>
        <Button
          type="button"
          size="sm"
          variant="outline"
          className={`h-8 min-w-0 w-fit max-w-full justify-between gap-2 px-2 text-xs ${className ?? ""}`}
          disabled={models.length === 0}
        >
          <span className="flex min-w-0 items-center gap-1">
            {selectedModel ? (
              <ModelProviderBadge model={selectedModel} credentials={credentials} />
            ) : null}
            <span className="truncate">{selectedModel?.name ?? t("ai.modelSelect")}</span>
            {selectedModel ? (
              <span className="shrink-0 text-muted-foreground">· {reasoningLabel}</span>
            ) : null}
          </span>
          <MdExpandMore className="shrink-0 text-sm" />
        </Button>
      </PopoverTrigger>
      <PopoverContent className="w-80 p-0" align="start">
        <Command>
          <CommandInput placeholder={t("ai.searchModels")} className="text-xs" />
          <CommandList className="max-h-64 terminal-scroll">
            <CommandEmpty>{t("ai.noModelMatches")}</CommandEmpty>
            <CommandGroup heading={t("ai.reasoning")}>
              {reasoningOptions.map((effort) => (
                <CommandItem
                  key={effort}
                  value={`${t(`ai.reasoningEffort.${effort}`)} ${effort}`}
                  onSelect={() => onSelectReasoningEffort(effort)}
                >
                  <MdCheck
                    className={`text-sm ${selectedReasoningEffort === effort ? "opacity-100" : "opacity-0"}`}
                  />
                  <span className="min-w-0 flex-1 truncate">
                    {t(`ai.reasoningEffort.${effort}`)}
                  </span>
                </CommandItem>
              ))}
            </CommandGroup>
            <CommandGroup heading={t("ai.models")}>
              {models.map((model) => {
                const providerLabel = getModelProviderLabel(model, credentials);
                return (
                  <CommandItem
                    key={model.id}
                    value={`${model.name} ${providerLabel} ${model.id}`}
                    onSelect={() => {
                      onSelect(model);
                      onOpenChange(false);
                    }}
                  >
                    <MdCheck
                      className={`text-sm ${selectedModel?.id === model.id ? "opacity-100" : "opacity-0"}`}
                    />
                    <ModelProviderBadge model={model} credentials={credentials} />
                    <span className="min-w-0 flex-1 truncate">{model.name}</span>
                    {providerLabel ? (
                      <span className="shrink-0 text-[0.625rem] text-muted-foreground">
                        {providerLabel}
                      </span>
                    ) : null}
                  </CommandItem>
                );
              })}
            </CommandGroup>
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  );
}
