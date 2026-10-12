[Rainmeter]
RightMouseUpAction=[]
Group=Monterey

[ThemeAsNumber]
Measure=String
String=#Theme#
DynamicVariables=1
Substitute="Light":"1","Dark":"2","Auto":"3","Color":"4","Blur":"5"

[BackgroundMeter]
Meter = Shape
DynamicVariables = 1
Shape = Rectangle 0, 0, #WidgetWidth#, #WidgetHeight#, #WidgetRadius#, #WidgetRadius# | Fill Color #BackgroundColor# | StrokeWidth 0
Padding = #WidgetPadding#, #WidgetPadding#, #WidgetPadding#, #WidgetPadding#

[Variables]
Context=Empty
WidgetRadius = #RadiusBase#
@Include1=#@#Variables\Global.inc
@Include2=#@#Languages\#Language#\Widgets\#WidgetName#.inc
@Include3=#@#Languages\#Language#\Widget.inc
@Include4=#@#Scripts\Themes\#Theme#.inc
@Include5=#@#Scripts\Sizes\Large.inc
@Include6=#@#Scripts\Includes\Context.inc
@Include7=#@#Variables\#WidgetName#.inc
@Include8=#@#Variables\Layout.inc

WidgetCenterX = (#WidgetPadding# + #WidgetWidth# / 2)
WidgetCenterY = (#WidgetPadding# + #WidgetHeight# / 2)

[IconMeter]
Meter=Image
ImageName=#@#Images\Icons\#WidgetName#.png
X=(#WidgetPadding# + #WidgetWidth# * 0.06)
Y=(#WidgetPadding# + #WidgetHeight# * 0.06)
W=(#WidgetWidth# * 0.1)
H=(#WidgetHeight# * 0.1)

[Title]
Meter=String
Text=[#t[#WidgetName]]
FontSize=(#WidgetWidth# * 0.035)
FontFace=#FontFace#
FontColor=#ForegroundColor#
DynamicVariables=1
FontWeight=600
AntiAlias=1
X=(#WidgetPadding# + #WidgetWidth# * 0.2)
Y=(#WidgetPadding# + #WidgetHeight# * 0.08)
W=(#WidgetWidth# * 0.5)
ClipString=1

[SaveText]
Meter=String
Text=#tSave#
FontFace=#FontFace#
FontSize=(#WidgetWidth# * 0.03)
FontColor=#ForegroundColor#
DynamicVariables=1
AntiAlias=1
StringAlign=Right
X=(#WidgetPadding# + #WidgetWidth# * 0.92)
Y=(#WidgetPadding# + #WidgetHeight# * 0.08)
LeftMouseUpAction=[!ActivateConfig "#CURRENTCONFIG#" "[#[#WidgetName]WidgetSize].ini"]
SolidColor=#GrayColor#
Padding=12,2,12,2

[Description]
Meter=String
Text=[#t[#WidgetName]Description]
FontSize=(#WidgetWidth# * 0.0325)
FontFace=#FontFace#
FontColor=128,128,128
FontWeight=300
AntiAlias=1
X=(#WidgetPadding# + #WidgetWidth# * 0.05)
Y=(#WidgetPadding# + #WidgetHeight# * 0.19)
W=(#WidgetWidth#*0.87)
ClipString=1
