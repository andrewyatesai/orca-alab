import type { JSX } from 'react'
import type { LucideIcon } from 'lucide-react'
import {
  CheckCircle2,
  Eye,
  ListRestart,
  MonitorSmartphone,
  MousePointer2,
  ShieldAlert,
  Smartphone,
  TerminalSquare
} from 'lucide-react'
import { translate } from '@/i18n/i18n'

export function MobileEmulatorsWorkflowVisual(): JSX.Element {
  return (
    <div
      className="w-full overflow-hidden rounded-xl border border-border bg-card shadow-xs"
      data-feature-wall-emulator-visual="true"
      aria-hidden
    >
      <div className="flex items-center gap-2 border-b border-border bg-muted/25 px-4 py-3">
        <MonitorSmartphone className="size-4 text-muted-foreground" />
        <p className="text-xs font-semibold">
          {translate(
            'auto.components.feature.wall.MobileEmulatorsWorkflowVisual.m140000001',
            'Workspace app test loop'
          )}
        </p>
        <span className="ml-auto rounded-full border border-border bg-background px-2 py-0.5 text-[11px] font-medium text-muted-foreground">
          {translate(
            'auto.components.feature.wall.MobileEmulatorsWorkflowVisual.m140000002',
            'Exact device selected'
          )}
        </span>
      </div>

      <div className="grid gap-3 p-3 sm:grid-cols-2">
        <PlatformLane
          platform={translate(
            'auto.components.feature.wall.MobileEmulatorsWorkflowVisual.m140000003',
            'iOS Simulator'
          )}
          host={translate(
            'auto.components.feature.wall.MobileEmulatorsWorkflowVisual.m140000004',
            'Local Mac · Xcode required'
          )}
          skill="orca-emulator"
          view={translate(
            'auto.components.feature.wall.MobileEmulatorsWorkflowVisual.m140000005',
            'Live Orca emulator pane'
          )}
          device="iPhone 17 Pro"
          accent="ios"
          actions={[
            translate(
              'auto.components.feature.wall.MobileEmulatorsWorkflowVisual.m140000006',
              'Attach workspace device'
            ),
            translate(
              'auto.components.feature.wall.MobileEmulatorsWorkflowVisual.m140000007',
              'Inspect accessibility'
            ),
            translate(
              'auto.components.feature.wall.MobileEmulatorsWorkflowVisual.m140000008',
              'Tap, type, gesture'
            )
          ]}
        />
        <PlatformLane
          platform={translate(
            'auto.components.feature.wall.MobileEmulatorsWorkflowVisual.m140000009',
            'Android emulator / device'
          )}
          host={translate(
            'auto.components.feature.wall.MobileEmulatorsWorkflowVisual.m140000010',
            'macOS · Linux · Windows'
          )}
          skill="orca-emulator-android"
          view={translate(
            'auto.components.feature.wall.MobileEmulatorsWorkflowVisual.m150000001',
            'Live Orca emulator pane'
          )}
          device="emulator-5554"
          accent="android"
          actions={[
            translate(
              'auto.components.feature.wall.MobileEmulatorsWorkflowVisual.m140000012',
              'Discover booted ADB device'
            ),
            translate(
              'auto.components.feature.wall.MobileEmulatorsWorkflowVisual.m150000002',
              'Stream, install, launch, inspect logs'
            ),
            translate(
              'auto.components.feature.wall.MobileEmulatorsWorkflowVisual.m140000014',
              'Tap, type, verify'
            )
          ]}
        />
      </div>

      <RecoveryTrace />

      <div className="flex items-start gap-2 border-t border-border px-4 py-2.5 text-[11px] leading-relaxed text-muted-foreground">
        <ShieldAlert className="mt-0.5 size-3.5 shrink-0" />
        <span>
          {translate(
            'auto.components.feature.wall.MobileEmulatorsWorkflowVisual.m140000018',
            'iOS control stays on the Mac that owns Simulator. Emulator actions can change the running app and its test data, so inspect the target before acting.'
          )}
        </span>
      </div>
    </div>
  )
}

function PlatformLane(props: {
  platform: string
  host: string
  skill: string
  view: string
  device: string
  accent: 'ios' | 'android'
  actions: readonly string[]
}): JSX.Element {
  return (
    <section className="overflow-hidden rounded-lg border border-border bg-background/70">
      <div className="flex items-center gap-2 border-b border-border px-3 py-2.5">
        <span className="flex size-8 items-center justify-center rounded-md border border-border bg-card">
          <Smartphone className="size-4 text-muted-foreground" />
        </span>
        <span className="min-w-0">
          <span className="block truncate text-xs font-semibold">{props.platform}</span>
          <span className="block truncate text-[11px] text-muted-foreground">{props.host}</span>
        </span>
      </div>

      <div className="grid grid-cols-[88px_minmax(0,1fr)] gap-3 p-2.5">
        <DevicePreview accent={props.accent} device={props.device} />
        <div className="min-w-0 space-y-2">
          <Fact icon={TerminalSquare} label={props.skill} />
          <Fact icon={Eye} label={props.view} />
          {props.actions.map((action, index) => (
            <div key={action} className="flex items-start gap-1.5 text-[11px]">
              <span className="mt-px flex size-3.5 shrink-0 items-center justify-center rounded-full border border-border text-[11px] text-muted-foreground">
                {index + 1}
              </span>
              <span className="leading-relaxed">{action}</span>
            </div>
          ))}
        </div>
      </div>
    </section>
  )
}

function DevicePreview(props: { accent: 'ios' | 'android'; device: string }): JSX.Element {
  return (
    <div>
      <div className="mx-auto w-[72px] rounded-[14px] border-2 border-foreground/25 bg-card p-1.5 shadow-xs">
        <div className="mx-auto mb-1 h-1 w-5 rounded-full bg-muted-foreground/35" />
        <div className="flex h-24 flex-col overflow-hidden rounded-[8px] border border-border bg-muted/35 p-2">
          <div className="flex items-center gap-1">
            <span
              className={
                props.accent === 'ios'
                  ? 'size-2 rounded-full bg-status-info'
                  : 'size-2 rounded-full bg-status-success'
              }
            />
            <span className="h-1.5 flex-1 rounded-full bg-muted-foreground/20" />
          </div>
          <div className="mt-3 space-y-1.5">
            <span className="block h-2 rounded bg-foreground/10" />
            <span className="block h-2 w-3/4 rounded bg-foreground/10" />
          </div>
          <div className="mt-auto flex items-center justify-center rounded-md border border-border bg-background py-2">
            <MousePointer2 className="size-3.5 text-muted-foreground" />
          </div>
        </div>
      </div>
      <p className="mt-1.5 truncate text-center font-mono text-[11px] text-muted-foreground">
        {props.device}
      </p>
    </div>
  )
}

function Fact(props: { icon: LucideIcon; label: string }): JSX.Element {
  const Icon = props.icon
  return (
    <div className="flex items-center gap-1.5 text-[11px] text-muted-foreground">
      <Icon className="size-3 shrink-0" />
      <span className="truncate font-mono">{props.label}</span>
    </div>
  )
}

function RecoveryTrace(): JSX.Element {
  return (
    <div
      className="grid gap-px border-t border-border bg-border sm:grid-cols-4"
      data-emulator-recovery-flow="stale-target-explicit-retry-action-verified"
    >
      <div
        className="flex min-h-24 items-start gap-2 bg-card p-3"
        data-emulator-recovery-stage="stale-target"
      >
        <ShieldAlert className="mt-0.5 size-3.5 shrink-0 text-muted-foreground" />
        <p className="text-[11px] leading-relaxed">
          {translate(
            'auto.components.feature.wall.MobileEmulatorsWorkflowVisual.m140000015',
            'No active or stale target'
          )}
        </p>
      </div>
      <div
        className="min-h-24 bg-card p-3"
        data-emulator-recovery-stage="explicit-device-retry"
        data-emulator-retry-device="emulator-5554"
      >
        <div className="flex items-start gap-2">
          <ListRestart className="mt-0.5 size-3.5 shrink-0 text-muted-foreground" />
          <div className="min-w-0 text-[11px] leading-relaxed">
            <p className="text-muted-foreground">
              {translate(
                'auto.components.feature.wall.MobileEmulatorsWorkflowVisual.m140000016',
                'List, attach, or boot'
              )}
            </p>
            <p className="font-medium">
              {translate(
                'auto.components.feature.wall.MobileEmulatorsWorkflowVisual.m140000017',
                'Retry with explicit device ID'
              )}
            </p>
            <p className="truncate font-mono text-muted-foreground">
              {translate('auto.fw.mobileEmulators.deviceId', 'emulator-5554')}
            </p>
          </div>
        </div>
        <p className="mt-2 flex items-center gap-1 text-[11px] font-medium text-status-success">
          <CheckCircle2 className="size-3 shrink-0" />
          {translate(
            'auto.components.feature.wall.MobileEmulatorsWorkflowVisual.m150000003',
            'Device connected'
          )}
        </p>
      </div>
      <div
        className="flex min-h-24 items-start gap-2 bg-card p-3"
        data-emulator-recovery-stage="action"
        data-emulator-action="tap-type"
      >
        <MousePointer2 className="mt-0.5 size-3.5 shrink-0 text-muted-foreground" />
        <p className="text-[11px] font-medium leading-relaxed">
          {translate(
            'auto.components.feature.wall.MobileEmulatorsWorkflowVisual.m150000004',
            'Tap Email · type agent@example.com'
          )}
        </p>
      </div>
      <div
        className="min-h-24 bg-status-success-background p-3"
        data-emulator-recovery-stage="verified-result"
        data-emulator-result="profile-email-updated"
      >
        <p className="flex items-center gap-1.5 text-[11px] font-semibold text-status-success">
          <CheckCircle2 className="size-3.5 shrink-0" />
          {translate(
            'auto.components.feature.wall.MobileEmulatorsWorkflowVisual.m150000005',
            'Verified app state'
          )}
        </p>
        <p className="mt-2 break-words font-mono text-[11px] leading-relaxed">
          {translate(
            'auto.components.feature.wall.MobileEmulatorsWorkflowVisual.m150000006',
            'Profile email · agent@example.com'
          )}
        </p>
      </div>
    </div>
  )
}
