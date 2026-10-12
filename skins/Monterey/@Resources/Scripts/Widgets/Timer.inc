[Variables]
DefaultTimer=#VisibleTime#

[VisibleHours]
Measure=Time
Timestamp=#VisibleTime#
DynamicVariables=1
UpdateDivider=1
Format=%H
OnChangeAction=[!UpdateMeter HoursMeter][!Redraw]

[VisibleMinutes]
Measure=Time
Timestamp=#VisibleTime#
DynamicVariables=1
UpdateDivider=1
Format=%M
OnChangeAction=[!UpdateMeter MinutesMeter][!Redraw]

[VisibleSeconds]
Measure=Time
Timestamp=#VisibleTime#
DynamicVariables=1
UpdateDivider=1
Format=%S
OnChangeAction=[!UpdateMeter SecondsMeter][!Redraw]

[IsStopped]
Measure=Calc
Group=Measures
Formula=(#StartTime# = -1)
DynamicVariables=1

[IsPaused]
Measure=Calc
Group=Measures
Formula=(#PauseTime# <> -1)
DynamicVariables=1

[IsStarted]
Measure=Calc
Group=Measures
Formula=(#StartTime# <> -1) && (#PauseTime# = -1)
DynamicVariables=1

[TrueNow]
Measure=Time
AverageSize=10
UpdateDivider=1

[EndTime]
Measure=Time
Group=Measures
DynamicVariables=1
Timestamp=#EndTime#
Format=🔔 %H:%M

[TimeLeft]
Measure=Calc
Group=Measures
Formula=Max(0, #EndTime# - [TrueNow:1] * [IsStarted] - #PauseTime# * [IsPaused])
Disabled=([IsStopped] = 1) 
OnUpdateAction=[!SetVariable VisibleTime [TimeLeft:1]]
IfCondition=([TimeLeft] = 0)
IfTrueAction=[PlayLoop "#@#Sounds\radar.wav"][!UpdateMeterGroup Meters]
DynamicVariables=1
UpdateDivider=1

[Percentage]
Measure=Calc
Group=Measures
Formula=([TimeLeft:1] / (#EndTime# - #StartTime#))
Disabled=([IsStopped] = 1) 
DynamicVariables=1
UpdateDivider=1

[Lua]
Measure=Script
ScriptFile=#@#Scripts\Widgets\Timer.lua