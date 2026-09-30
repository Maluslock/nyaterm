import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { NumberInput } from "@/components/ui/number-input";
import { Select, SelectContent, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { cn } from "@/lib/utils";

type SettingMetaProps = {
  label: string;
  desc?: string;
};

type SettingFieldShellProps = SettingMetaProps & {
  children: React.ReactNode;
  className?: string;
  controlClassName?: string;
};

type SettingSectionProps = {
  title?: React.ReactNode;
  desc?: string;
  action?: React.ReactNode;
  children: React.ReactNode;
  className?: string;
  headerRowClassName?: string;
  contentClassName?: string;
};

function SettingMeta({ label, desc }: SettingMetaProps) {
  return (
    <div className="min-w-0">
      <Label className="text-sm font-medium leading-5">{label}</Label>
      {desc && <p className="mt-1 text-xs leading-5 text-muted-foreground">{desc}</p>}
    </div>
  );
}

function SettingFieldShell({
  label,
  desc,
  children,
  className,
  controlClassName,
}: SettingFieldShellProps) {
  return (
    <div className={cn("space-y-3", className)}>
      <SettingMeta label={label} desc={desc} />
      <div className={cn("min-w-0 max-w-xl", controlClassName)}>{children}</div>
    </div>
  );
}

export function SettingSection({
  title,
  desc,
  action,
  children,
  className,
  headerRowClassName,
  contentClassName,
}: SettingSectionProps) {
  return (
    <section
      className={cn("min-w-0 rounded-xl border border-border/70 bg-card/60 shadow-xs", className)}
    >
      {(title || desc || action) && (
        <div className="flex flex-col gap-3 border-b border-border/60 px-4 py-4 sm:px-5">
          <div
            className={cn(
              "flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between",
              headerRowClassName,
            )}
          >
            {(title || desc) && (
              <div className="min-w-0">
                {title && <h3 className="text-sm font-semibold leading-5">{title}</h3>}
                {desc && <p className="mt-1 text-xs leading-5 text-muted-foreground">{desc}</p>}
              </div>
            )}
            {action && <div className="flex shrink-0 items-center">{action}</div>}
          </div>
        </div>
      )}
      <div className={cn("min-w-0 space-y-4 px-4 py-4 sm:px-5 sm:py-5", contentClassName)}>
        {children}
      </div>
    </section>
  );
}

export function SettingFieldGrid({
  children,
  className,
}: {
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <div className={cn("grid min-w-0 gap-4 lg:grid-cols-2 lg:gap-x-6", className)}>{children}</div>
  );
}

export function SettingRow({
  label,
  desc,
  children,
}: {
  label: string;
  desc?: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex flex-col gap-3 sm:grid sm:grid-cols-[minmax(0,1fr)_auto] sm:items-start sm:gap-x-6">
      <SettingMeta label={label} desc={desc} />
      <div className="flex min-w-0 items-center justify-start gap-2 sm:justify-end">{children}</div>
    </div>
  );
}

export function SettingInput({
  label,
  desc,
  fieldClassName,
  controlClassName,
  className,
  ...inputProps
}: {
  label: string;
  desc?: string;
  fieldClassName?: string;
  controlClassName?: string;
} & React.ComponentProps<typeof Input>) {
  return (
    <SettingFieldShell
      label={label}
      desc={desc}
      className={fieldClassName}
      controlClassName={controlClassName}
    >
      <Input className={cn("w-full text-sm", className)} {...inputProps} />
    </SettingFieldShell>
  );
}

export function SettingNumberInput({
  label,
  desc,
  value,
  onChange,
  disabled,
  min,
  max,
  step,
  className,
  fieldClassName,
  controlClassName,
}: {
  label: string;
  desc?: string;
  value: number;
  onChange: (v: number) => void;
  disabled?: boolean;
  min?: number;
  max?: number;
  step?: number;
  className?: string;
  fieldClassName?: string;
  controlClassName?: string;
}) {
  return (
    <SettingFieldShell
      label={label}
      desc={desc}
      className={fieldClassName}
      controlClassName={controlClassName}
    >
      <NumberInput
        value={value}
        onChange={onChange}
        disabled={disabled}
        min={min}
        max={max}
        step={step}
        className={cn("w-full", className)}
      />
    </SettingFieldShell>
  );
}

export function SettingSelect({
  label,
  desc,
  value,
  onValueChange,
  disabled,
  children,
  fieldClassName,
  controlClassName,
  triggerClassName,
}: {
  label: string;
  desc?: string;
  value: string;
  onValueChange: (v: string) => void;
  disabled?: boolean;
  children: React.ReactNode;
  fieldClassName?: string;
  controlClassName?: string;
  triggerClassName?: string;
}) {
  return (
    <SettingFieldShell
      label={label}
      desc={desc}
      className={fieldClassName}
      controlClassName={controlClassName}
    >
      <Select value={value} onValueChange={onValueChange} disabled={disabled}>
        <SelectTrigger className={cn("w-full text-sm", triggerClassName)}>
          <SelectValue />
        </SelectTrigger>
        <SelectContent>{children}</SelectContent>
      </Select>
    </SettingFieldShell>
  );
}

export function SettingSwitch({
  checked,
  disabled,
  onChange,
  ...switchProps
}: {
  checked: boolean;
  disabled?: boolean;
  onChange: (v: boolean) => void;
} & Omit<
  React.ComponentProps<typeof Switch>,
  "checked" | "disabled" | "onChange" | "onCheckedChange"
>) {
  return (
    <Switch checked={checked} disabled={disabled} onCheckedChange={onChange} {...switchProps} />
  );
}
