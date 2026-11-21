import { AnimatePresence, motion } from "motion/react";
import type { CSSProperties, ReactNode } from "react";
import {
  Button as AriaButton,
  type ButtonProps as AriaButtonProps,
  Switch as AriaSwitch,
  Dialog,
  FieldError,
  Input,
  Label,
  Modal,
  ModalOverlay,
  Radio,
  RadioGroup,
  Text,
  TextField,
} from "react-aria-components";
import type { BatteryState } from "../bridge";
import { Icon, type IconName } from "../design/icons";
import { useStore } from "../state/store";
import s from "./ui.module.css";

const cx = (...c: (string | false | undefined | null)[]) => c.filter(Boolean).join(" ");

export function Button({
  variant = "quiet",
  size = "md",
  icon,
  iconOnly,
  children,
  className,
  ...rest
}: AriaButtonProps & {
  variant?: "primary" | "quiet" | "ghost" | "danger";
  size?: "sm" | "md" | "lg";
  icon?: IconName;
  iconOnly?: boolean;
  children?: ReactNode;
  className?: string;
}) {
  return (
    <AriaButton
      {...rest}
      className={cx(s.button, s[variant], size !== "md" && s[size], iconOnly && s.iconOnly, className)}
    >
      {icon && <Icon name={icon} size={size === "sm" ? 16 : 18} />}
      {iconOnly ? <span className="visually-hidden">{children}</span> : children}
    </AriaButton>
  );
}

export function Switch({
  isSelected,
  onChange,
  children,
  isDisabled,
  accent,
}: {
  isSelected: boolean;
  onChange: (v: boolean) => void;
  children: ReactNode;
  isDisabled?: boolean;
  accent?: string;
}) {
  return (
    <AriaSwitch
      isSelected={isSelected}
      onChange={onChange}
      isDisabled={isDisabled}
      className={s.switch}
      style={accent ? ({ "--accent": accent } as CSSProperties) : undefined}
    >
      <div className={s.track}>
        <span className={s.thumb} />
      </div>
      {children}
    </AriaSwitch>
  );
}

export function Field({
  label,
  value,
  onChange,
  hint,
  error,
  autoFocus,
  maxLength,
  inputMode,
  placeholder,
  onSubmit,
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
  hint?: string;
  error?: string | null;
  autoFocus?: boolean;
  maxLength?: number;
  inputMode?: "text" | "numeric";
  placeholder?: string;
  onSubmit?: () => void;
}) {
  return (
    <TextField
      className={s.field}
      value={value}
      onChange={onChange}
      isInvalid={!!error}
      autoFocus={autoFocus}
      maxLength={maxLength}
      inputMode={inputMode}
      onKeyDown={(e) => {
        if (e.key === "Enter") onSubmit?.();
      }}
    >
      <Label className={s.label}>{label}</Label>
      <Input className={s.input} placeholder={placeholder} />
      {hint && !error && (
        <Text slot="description" className={s.hint}>
          {hint}
        </Text>
      )}
      <FieldError className={s.error}>{error}</FieldError>
    </TextField>
  );
}

export function Segmented<T extends string>({
  label,
  value,
  onChange,
  options,
}: {
  label: string;
  value: T;
  onChange: (v: T) => void;
  options: { value: T; label: string; icon?: IconName }[];
}) {
  return (
    <RadioGroup
      aria-label={label}
      value={value}
      onChange={(v) => onChange(v as T)}
      orientation="horizontal"
      className={s.segmented}
    >
      {options.map((o) => (
        <Radio key={o.value} value={o.value} className={s.segment}>
          {o.icon && <Icon name={o.icon} size={16} />}
          {o.label}
        </Radio>
      ))}
    </RadioGroup>
  );
}

export function Sheet({
  isOpen,
  onClose,
  label,
  narrow,
  children,
  isDismissable = true,
}: {
  isOpen: boolean;
  onClose: () => void;
  label: string;
  narrow?: boolean;
  children: ReactNode;
  isDismissable?: boolean;
}) {
  return (
    <ModalOverlay
      isOpen={isOpen}
      onOpenChange={(o) => !o && onClose()}
      isDismissable={isDismissable}
      className={s.overlay}
    >
      <Modal className={cx(s.sheet, narrow && s.sheetNarrow)}>
        <Dialog aria-label={label} style={{ outline: "none" }}>
          {children}
        </Dialog>
      </Modal>
    </ModalOverlay>
  );
}

export function Toasts() {
  const toasts = useStore((st) => st.toasts);
  const dismiss = useStore((st) => st.dismissToast);
  return (
    <div className={s.toasts} role="status" aria-live="polite">
      <AnimatePresence>
        {toasts.map((t) => (
          <motion.div
            key={t.id}
            className={s.toast}
            data-tone={t.tone}
            initial={{ opacity: 0, y: 12, scale: 0.98 }}
            animate={{ opacity: 1, y: 0, scale: 1 }}
            exit={{ opacity: 0, y: 6 }}
            transition={{ type: "spring", stiffness: 420, damping: 32 }}
          >
            <Icon name={t.tone === "danger" ? "warning" : t.tone === "ok" ? "check" : "info"} size={18} />
            <span style={{ flex: 1 }}>{t.text}</span>
            <Button variant="ghost" size="sm" iconOnly icon="close" onPress={() => dismiss(t.id)}>
              Dismiss
            </Button>
          </motion.div>
        ))}
      </AnimatePresence>
    </div>
  );
}

export function BatteryMeter({ battery, accent }: { battery: BatteryState | null; accent?: string }) {
  if (!battery) return null;
  const low = battery.percent <= 15 && !battery.charging;
  return (
    <span
      className={cx(s.battery, low && s.batteryLow)}
      style={accent ? ({ "--accent": accent } as CSSProperties) : undefined}
      role="img"
      aria-label={`Battery ${battery.percent} percent${battery.charging ? ", charging" : ""}`}
    >
      <span className={s.batteryTrack}>
        <span className={s.batteryFill} style={{ width: `${battery.percent}%` }} />
      </span>
      {battery.percent}%{battery.charging && <Icon name="bolt" size={13} />}
    </span>
  );
}
