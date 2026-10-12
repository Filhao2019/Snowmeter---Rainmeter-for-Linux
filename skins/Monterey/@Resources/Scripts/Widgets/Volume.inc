[Rainmeter]
OnRefreshAction=[!CommandMeasure VolumeMeasureAnimation "Execute [&ActionList]"][!CommandMeasure VolumeApp1Animation "Execute [&ActionList]"][!CommandMeasure VolumeApp2Animation "Execute [&ActionList]"][!CommandMeasure VolumeApp3Animation "Execute [&ActionList]"][!CommandMeasure VolumeApp4Animation "Execute [&ActionList]"][!CommandMeasure VolumeApp5Animation "Execute [&ActionList]"]

[Variables]
VolumeTotal=0
VolumeApp1=0
VolumeApp2=0
VolumeApp3=0
VolumeApp4=0
VolumeApp5=0

[VolumeMeasure]
Measure=Plugin
Plugin=Win7AudioPlugin
OnChangeAction=[!UpdateMeasure VolumeChangedAction][!CommandMeasure VolumeMeasureAnimation "Execute [&ActionList]"]
DynamicVariables=1
UpdateDivider=1

[ActionList]
Measure=Calc
Formula=1+#BatterySaverMode#

[VolumeMeasureAnimation]
Measure=Plugin
Plugin=ActionTimer
ActionList1=Repeat AddStep, 100, 10 | Update
ActionList2=Update
AddStep=[!SetVariable VolumeTotal ([#VolumeTotal]+([&VolumeMeasure:]-[#VolumeTotal])/2)][!UpdateMeasure VolumeTotal][!UpdateMeterGroup VolumeTotal][!Redraw]
Update=[!SetVariable VolumeTotal [&VolumeMeasure:]][!UpdateMeasure VolumeTotal][!UpdateMeterGroup VolumeTotal][!Redraw]
DynamicVariables=1

[VolumeTotal]
Measure=Calc
Formula=Round(Clamp(#VolumeTotal#, 0, 100))
MinValue=0
MaxValue=100
DynamicVariables=1

[VolumeChangedAction]
Measure=Calc
DynamicVariables=1
IfCondition=(VolumeMeasure = -1)
IfTrueAction=[!SetVariable VolumeIcon "-1"][!SetVariable VolumeMute "-1"]
IfCondition2=(VolumeMeasure = 0)
IfTrueAction2=[!SetVariable VolumeIcon "0"][!SetVariable VolumeMute "0"]
IfCondition3=(VolumeMeasure > 0) && (VolumeMeasure <= 33)
IfTrueAction3=[!SetVariable VolumeIcon "1"][!SetVariable VolumeMute "0"]
IfCondition4=(VolumeMeasure > 33) && (VolumeMeasure <= 66)
IfTrueAction4=[!SetVariable VolumeIcon "2"][!SetVariable VolumeMute "0"]
IfCondition5=(VolumeMeasure > 66)
IfTrueAction5=[!SetVariable VolumeIcon "3"][!SetVariable VolumeMute "0"]

[AppVolume]
Measure=Plugin
Plugin=AppVolume
IgnoreSystemSound=1
OnChangeAction=[!UpdateMeasureGroup AppVolume]
UpdateDivider=10

[AppVolume1]
Measure=Plugin
Group=AppVolume
Plugin=AppVolume
Parent=AppVolume
OnChangeAction=[!UpdateMeasure AppVolume1ChangedAction][!CommandMeasure VolumeApp1Animation "Execute [&ActionList]"]
UpdateDivider=1
Index=1
Substitute=".exe":""

[VolumeApp1Animation]
Measure=Plugin
Plugin=ActionTimer
ActionList1=Repeat AddStep, 100, 10 | Update
ActionList2=Update
AddStep=[!SetVariable VolumeApp1 ([#VolumeApp1]+([&AppVolume1:]-[#VolumeApp1])/2)][!UpdateMeasure VolumeApp1][!UpdateMeterGroup VolumeApp1][!Redraw]
Update=[!SetVariable VolumeApp1 [&AppVolume1:]][!UpdateMeasure VolumeApp1][!UpdateMeterGroup VolumeApp1][!Redraw]
DynamicVariables=1

[VolumeApp1]
Measure=Calc
Formula=Round(Clamp(#VolumeApp1# * 100, 0, 100))
MinValue=0
MaxValue=100
DynamicVariables=1

[AppVolume1ChangedAction]
Measure=Calc
IfCondition=(AppVolume1 = -1)
IfTrueAction=[!SetVariable AppVolume1Mute "-1"]
IfFalseAction=[!SetVariable AppVolume1Mute "0"]
OnUpdateAction=[!UpdateMeterGroup Meters][!Redraw]
DynamicVariables=1

[AppVolume2]
Measure=Plugin
Group=AppVolume
Plugin=AppVolume
Parent=AppVolume
OnChangeAction=[!UpdateMeasure AppVolume2ChangedAction][!CommandMeasure VolumeApp2Animation "Execute [&ActionList]"]
UpdateDivider=1
Index=2
Substitute=".exe":""

[VolumeApp2Animation]
Measure=Plugin
Plugin=ActionTimer
ActionList1=Repeat AddStep, 100, 10 | Update
ActionList2=Update
AddStep=[!SetVariable VolumeApp2 ([#VolumeApp2]+([&AppVolume2:]-[#VolumeApp2])/2)][!UpdateMeasure VolumeApp2][!UpdateMeterGroup VolumeApp2][!Redraw]
Update=[!SetVariable VolumeApp2 [&AppVolume2:]][!UpdateMeasure VolumeApp2][!UpdateMeterGroup VolumeApp2][!Redraw]
DynamicVariables=1

[VolumeApp2]
Measure=Calc
Formula=Round(Clamp(#VolumeApp2# * 100, 0, 100))
MinValue=0
MaxValue=100
DynamicVariables=1

[AppVolume2ChangedAction]
Measure=Calc
IfCondition=(AppVolume2 = -1)
IfTrueAction=[!SetVariable AppVolume2Mute "-1"]
IfFalseAction=[!SetVariable AppVolume2Mute "0"]
OnUpdateAction=[!UpdateMeterGroup Meters][!Redraw]
DynamicVariables=1

[AppVolume3]
Measure=Plugin
Group=AppVolume
Plugin=AppVolume
Parent=AppVolume
OnChangeAction=[!UpdateMeasure AppVolume3ChangedAction][!CommandMeasure VolumeApp3Animation "Execute [&ActionList]"]
UpdateDivider=1
Index=3
Substitute=".exe":""

[VolumeApp3Animation]
Measure=Plugin
Plugin=ActionTimer
ActionList1=Repeat AddStep, 100, 10 | Update
ActionList2=Update
AddStep=[!SetVariable VolumeApp3 ([#VolumeApp3]+([&AppVolume3:]-[#VolumeApp3])/2)][!UpdateMeasure VolumeApp3][!UpdateMeterGroup VolumeApp3][!Redraw]
Update=[!SetVariable VolumeApp3 [&AppVolume3:]][!UpdateMeasure VolumeApp3][!UpdateMeterGroup VolumeApp3][!Redraw]
DynamicVariables=1

[VolumeApp3]
Measure=Calc
Formula=Round(Clamp(#VolumeApp3# * 100, 0, 100))
MinValue=0
MaxValue=100
DynamicVariables=1

[AppVolume3ChangedAction]
Measure=Calc
IfCondition=(AppVolume3 = -1)
IfTrueAction=[!SetVariable AppVolume3Mute "-1"]
IfFalseAction=[!SetVariable AppVolume3Mute "0"]
OnUpdateAction=[!UpdateMeterGroup Meters][!Redraw]
DynamicVariables=1

[AppVolume4]
Measure=Plugin
Group=AppVolume
Plugin=AppVolume
Parent=AppVolume
OnChangeAction=[!UpdateMeasure AppVolume4ChangedAction][!CommandMeasure VolumeApp4Animation "Execute [&ActionList]"]    
UpdateDivider=1
Index=4
Substitute=".exe":""

[VolumeApp4Animation]
Measure=Plugin
Plugin=ActionTimer
ActionList1=Repeat AddStep, 100, 10 | Update
ActionList2=Update
AddStep=[!SetVariable VolumeApp4 ([#VolumeApp4]+([&AppVolume4:]-[#VolumeApp4])/2)][!UpdateMeasure VolumeApp4][!UpdateMeterGroup VolumeApp4][!Redraw]
Update=[!SetVariable VolumeApp4 [&AppVolume4:]][!UpdateMeasure VolumeApp4][!UpdateMeterGroup VolumeApp4][!Redraw]
DynamicVariables=1

[VolumeApp4]
Measure=Calc
Formula=Round(Clamp(#VolumeApp4# * 100, 0, 100))
MinValue=0
MaxValue=100
DynamicVariables=1

[AppVolume4ChangedAction]
Measure=Calc
IfCondition=(AppVolume4 = -1)
IfTrueAction=[!SetVariable AppVolume4Mute "-1"]
IfFalseAction=[!SetVariable AppVolume4Mute "0"]
OnUpdateAction=[!UpdateMeterGroup Meters][!Redraw]
DynamicVariables=1

[AppVolume5]
Measure=Plugin
Group=AppVolume
Plugin=AppVolume
Parent=AppVolume
OnChangeAction=[!UpdateMeasure AppVolume5ChangedAction][!CommandMeasure VolumeApp5Animation "Execute [&ActionList]"]
UpdateDivider=1
Index=5
Substitute=".exe":""

[VolumeApp5Animation]
Measure=Plugin
Plugin=ActionTimer
ActionList1=Repeat AddStep, 100, 10 | Update
ActionList2=Update
AddStep=[!SetVariable VolumeApp5 ([#VolumeApp5]+([&AppVolume5:]-[#VolumeApp5])/2)][!UpdateMeasure VolumeApp5][!UpdateMeterGroup VolumeApp5][!Redraw]
Update=[!SetVariable VolumeApp5 [&AppVolume5:]][!UpdateMeasure VolumeApp5][!UpdateMeterGroup VolumeApp5][!Redraw]
DynamicVariables=1

[VolumeApp5]
Measure=Calc
Formula=Round(Clamp(#VolumeApp5# * 100, 0, 100))
MinValue=0
MaxValue=100
DynamicVariables=1

[AppVolume5ChangedAction]
Measure=Calc
IfCondition=(AppVolume5 = -1)
IfTrueAction=[!SetVariable AppVolume5Mute "-1"]
IfFalseAction=[!SetVariable AppVolume5Mute "0"]
OnUpdateAction=[!UpdateMeterGroup Meters][!Redraw]
DynamicVariables=1
