import { useTheme } from "@/contexts";
import { Header, Label, Slider, Button } from "@/components";
import { MonitorIcon, MoonIcon, SunIcon } from "lucide-react";
import { cn } from "@/lib/utils";

const THEME_OPTIONS = [
  {
    value: "light" as const,
    label: "Light",
    icon: SunIcon,
    description: "Light theme for better visibility in bright environments",
  },
  {
    value: "dark" as const,
    label: "Dark",
    icon: MoonIcon,
    description: "Dark theme for comfortable viewing in low light",
  },
  {
    value: "system" as const,
    label: "System",
    icon: MonitorIcon,
    description: "Follows your operating system appearance",
  },
];

export const Theme = () => {
  const { theme, resolvedTheme, transparency, setTheme, onSetTransparency } =
    useTheme();

  const activeOption =
    THEME_OPTIONS.find((option) => option.value === theme) ?? THEME_OPTIONS[2];
  const ActiveIcon = activeOption.icon;

  return (
    <div id="theme" className="relative space-y-3">
      <Header
        title="Theme Customization"
        description="Personalize your experience with custom theme and transparency settings"
        isMainTitle
      />

      {/* Appearance Toggle */}
      <div className="space-y-3">
        <div className="flex items-center justify-between gap-4">
          <div>
            <Label className="text-sm font-medium flex items-center gap-2">
              <ActiveIcon className="h-4 w-4" />
              {activeOption.label}
              {theme === "system" ? (
                <span className="text-xs font-normal text-muted-foreground">
                  ({resolvedTheme === "dark" ? "dark" : "light"})
                </span>
              ) : null}
            </Label>
            <p className="text-xs text-muted-foreground mt-1">
              {activeOption.description}
            </p>
          </div>

          <div className="flex items-center gap-1 rounded-xl border border-input/60 bg-muted/40 p-1">
            {THEME_OPTIONS.map((option) => (
              <Button
                key={option.value}
                size="sm"
                variant={theme === option.value ? "default" : "ghost"}
                onClick={() => setTheme(option.value)}
                title={option.description}
                aria-pressed={theme === option.value}
                className={cn(
                  "gap-1.5",
                  theme === option.value ? "" : "text-muted-foreground"
                )}
              >
                <option.icon className="h-4 w-4" />
                {option.label}
              </Button>
            ))}
          </div>
        </div>
      </div>

      {/* Transparency Slider */}
      <div className="space-y-2">
        <Header
          title="Window Transparency"
          description="Adjust the transparency level of the application window"
        />
        <div className="space-y-3">
          <div className="flex items-center gap-4 mt-4">
            <Slider
              value={[transparency]}
              onValueChange={(value: number[]) => onSetTransparency(value[0])}
              min={0}
              max={100}
              step={1}
              className="flex-1"
            />
          </div>

          <p className="text-xs text-muted-foreground/70">
            💡 Tip: Higher transparency lets you see through the window, perfect
            for dark overlay. Changes apply immediately.
          </p>
        </div>
      </div>
    </div>
  );
};
