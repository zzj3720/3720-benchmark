"use client";

import * as Collapsible from "@radix-ui/react-collapsible";
import * as DropdownMenu from "@radix-ui/react-dropdown-menu";
import * as Select from "@radix-ui/react-select";
import * as Slider from "@radix-ui/react-slider";
import { Check, ChevronDown, ChevronRight, ChevronUp, MoreHorizontal, type LucideIcon } from "lucide-react";
import type { ReactNode } from "react";

export function LiveSelect({ label, value, options, onChange, disabled, className = "" }: {
  label: string;
  value: string;
  options: { value: string; label: string }[];
  onChange: (value: string) => void;
  disabled?: boolean;
  className?: string;
}) {
  const selected = options.find(option => option.value === value);
  return (
    <Select.Root value={value} onValueChange={onChange} disabled={disabled}>
      <Select.Trigger className={`live-select-trigger ${className}`} aria-label={label} title={selected?.label}>
        <Select.Value><span className="live-select-value">{selected?.label ?? "请选择"}</span></Select.Value>
        <Select.Icon asChild><ChevronDown size={14} aria-hidden="true" /></Select.Icon>
      </Select.Trigger>
      <Select.Portal>
        <Select.Content className="live-select-content" position="popper" sideOffset={4} collisionPadding={8}>
          <Select.ScrollUpButton className="live-select-scroll"><ChevronUp size={14} aria-hidden="true" /></Select.ScrollUpButton>
          <Select.Viewport className="live-select-viewport">
            {options.map(option => (
              <Select.Item key={option.value} value={option.value} className="live-select-item" title={option.label}>
                <Select.ItemIndicator className="live-select-check"><Check size={14} aria-hidden="true" /></Select.ItemIndicator>
                <Select.ItemText>{option.label}</Select.ItemText>
              </Select.Item>
            ))}
          </Select.Viewport>
          <Select.ScrollDownButton className="live-select-scroll"><ChevronDown size={14} aria-hidden="true" /></Select.ScrollDownButton>
        </Select.Content>
      </Select.Portal>
    </Select.Root>
  );
}

export function LiveSlider({ value, max, onChange, disabled }: {
  value: number;
  max: number;
  onChange: (value: number) => void;
  disabled?: boolean;
}) {
  return (
    <Slider.Root className="live-slider" value={[value]} min={0} max={Math.max(1, max)} step={1} onValueChange={values => onChange(values[0])} disabled={disabled || max < 1}>
      <Slider.Track className="live-slider-track"><Slider.Range className="live-slider-range" /></Slider.Track>
      <Slider.Thumb className="live-slider-thumb" aria-label="选择有效回放画面" aria-valuetext={`第 ${value + 1} 帧，共 ${max + 1} 帧`} />
    </Slider.Root>
  );
}

export function LiveDisclosure({ title, className = "", defaultOpen = false, children }: {
  title: string;
  className?: string;
  defaultOpen?: boolean;
  children: ReactNode;
}) {
  return (
    <Collapsible.Root className={`live-disclosure ${className}`} defaultOpen={defaultOpen}>
      <Collapsible.Trigger className="live-disclosure-trigger"><ChevronRight size={14} aria-hidden="true" />{title}</Collapsible.Trigger>
      <Collapsible.Content className="live-disclosure-content">{children}</Collapsible.Content>
    </Collapsible.Root>
  );
}

export function LiveActionMenu({ items }: {
  items: { label: string; icon: LucideIcon; disabled?: boolean; onSelect: () => void; separator?: boolean }[];
}) {
  return (
    <DropdownMenu.Root>
      <DropdownMenu.Trigger className="live-menu-trigger" aria-label="更多回放操作" title="更多回放操作"><MoreHorizontal size={18} aria-hidden="true" /></DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content className="live-menu-content" sideOffset={4} collisionPadding={8} align="end">
          {items.map(({ icon: Icon, ...item }) => (
            <div key={item.label}>
              {item.separator && <DropdownMenu.Separator className="live-menu-separator" />}
              <DropdownMenu.Item className="live-menu-item" disabled={item.disabled} onSelect={item.onSelect}><Icon size={15} aria-hidden="true" /><span>{item.label}</span></DropdownMenu.Item>
            </div>
          ))}
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  );
}
