[OpenMeteoParser]
Measure=WebParser
URL=https://api.open-meteo.com/v1/forecast?latitude=#Latitude#&longitude=#Longitude#&temperature_unit=#Units#&current_weather=true&hourly=temperature_2m,weathercode&daily=temperature_2m_min,temperature_2m_max,weathercode&timezone=auto
UpdateDivider=1
UpdateRate=(60*30)
RegExp=(?U)(?>("error")|(?>"current_weather".*"temperature":#T#.*"weathercode":#T#.*"hourly".*"temperature_2m":\[#T##T##T##T##T##T##T##T##T##T##T##T##T##T##T##T##T##T##T##T##T##T##T##T##T##T##T##T##T##T#.*"weathercode":\[#T##T##T##T##T##T##T##T##T##T##T##T##T##T##T##T##T##T##T##T##T##T##T##T##T##T##T##T##T##T#.*"daily".*"temperature_2m_min":\[#T##T##T##T##T##T#.*"temperature_2m_max":\[#T##T##T##T##T##T#.*"weathercode":\[#T##T##T##T##T##T#))
FinishAction=[!UpdateMeasureGroup Measures][!UpdateMeterGroup Meters][!Redraw]
OnConnectErrorAction=[!SetVariable IsError "1"][!SetVariable ErrorCode "1"][!UpdateMeterGroup][!Redraw]

[Variables]
;Capture group
T=(.*)[,\}]
IsError=0
ErrorCode=-1
IconName="99":"thunder","96":"thunder","95":"thunder","85":"rain","82":"rain","81":"rain","80":"rain","77":"snow","75":"snow","73":"snow","71":"snow","67":"rain","66":"rain","65":"rain","63":"rain","61":"rain","57":"drizzle","56":"drizzle","55":"drizzle","53":"drizzle","51":"drizzle","48":"fog","45":"fog","3":"overcast","2":"cloudy","1":"sunny","0":"sunny"

[ErrorCode]
Measure=WebParser
URL=[OpenMeteoParser]
StringIndex=1
RegExpSubstitute=1
Substitute=".+":"1","^$":"0"

[SetIsError]
Measure=String
Group=Measures
IfCondition=[ErrorCode] > 0
IfTrueAction=[!SetVariable IsError 1][!SetVariable ErrorCode 0]
IfFalseAction=[!SetVariable IsError 0][!SetVariable ErrorCode -1]
IfConditionMode=1
DynamicVariables=1

[CurrentTempValue]
Measure=WebParser
URL=[OpenMeteoParser]
StringIndex=2

[CurrentTemp]
Measure=Calc
Group=Measures
Formula=Round([CurrentTempValue],0)
Substitute="-0":"0"
DynamicVariables=1

[CurrentDescriptionValue]
Measure=WebParser
URL=[OpenMeteoParser]
StringIndex=3

[CurrentDescription]
Measure=String
Group=Measures
String=[#tWeatherCode[&CurrentDescriptionValue]]
DynamicVariables=1
Substitute="[#tWeatherCode]":""

[CurrentIcon]
Measure=WebParser
URL=[OpenMeteoParser]
StringIndex=3
Substitute=#IconName#

[HourlyTime1]
Measure=Time
Group=Measures
DynamicVariables=1
Format=%H

[HourlyTempValue1]
Measure=WebParser
DynamicVariables=1
URL=[OpenMeteoParser]
StringIndex=(4+[HourlyTime1:])

[HourlyTemp1]
Measure=Calc
Group=Measures
Formula=Round([HourlyTempValue1],0)
Substitute="-0":"0"
DynamicVariables=1

[HourlyIcon1]
Measure=WebParser
DynamicVariables=1
URL=[OpenMeteoParser]
StringIndex=(34+[HourlyTime1:])
Substitute=#IconName#

[HourlyTime2]
Measure=Time
Group=Measures
TimeStamp=([HourlyTime1:Timestamp]+3600)
DynamicVariables=1
Format=%H

[HourlyTempValue2]
Measure=WebParser
DynamicVariables=1
URL=[OpenMeteoParser]
StringIndex=(4+[HourlyTime1:]+1)

[HourlyTemp2]
Measure=Calc
Group=Measures
Formula=Round([HourlyTempValue2],0)
Substitute="-0":"0"
DynamicVariables=1

[HourlyIcon2]
Measure=WebParser
DynamicVariables=1
URL=[OpenMeteoParser]
StringIndex=(34+[HourlyTime1:]+1)
Substitute=#IconName#

[HourlyTime3]
Measure=Time
Group=Measures
TimeStamp=([HourlyTime1:Timestamp]+2*3600)
DynamicVariables=1
Format=%H

[HourlyTempValue3]
Measure=WebParser
DynamicVariables=1
URL=[OpenMeteoParser]
StringIndex=(4+[HourlyTime1:]+2)

[HourlyTemp3]
Measure=Calc
Group=Measures
Formula=Round([HourlyTempValue3],0)
Substitute="-0":"0"
DynamicVariables=1

[HourlyIcon3]
Measure=WebParser
DynamicVariables=1
URL=[OpenMeteoParser]
StringIndex=(34+[HourlyTime1:]+2)
Substitute=#IconName#

[HourlyTime4]
Measure=Time
Group=Measures
TimeStamp=([HourlyTime1:Timestamp]+3*3600)
DynamicVariables=1
Format=%H

[HourlyTempValue4]
Measure=WebParser
DynamicVariables=1
URL=[OpenMeteoParser]
StringIndex=(4+[HourlyTime1:]+3)

[HourlyTemp4]
Measure=Calc
Group=Measures
Formula=Round([HourlyTempValue4],0)
Substitute="-0":"0"
DynamicVariables=1

[HourlyIcon4]
Measure=WebParser
DynamicVariables=1
URL=[OpenMeteoParser]
StringIndex=(34+[HourlyTime1:]+3)
Substitute=#IconName#

[HourlyTime5]
Measure=Time
Group=Measures
TimeStamp=([HourlyTime1:Timestamp]+4*3600)
DynamicVariables=1
Format=%H

[HourlyTempValue5]
Measure=WebParser
DynamicVariables=1
URL=[OpenMeteoParser]
StringIndex=(4+[HourlyTime1:]+4)

[HourlyTemp5]
Measure=Calc
Group=Measures
Formula=Round([HourlyTempValue5],0)
Substitute="-0":"0"
DynamicVariables=1

[HourlyIcon5]
Measure=WebParser
DynamicVariables=1
URL=[OpenMeteoParser]
StringIndex=(34+[HourlyTime1:]+4)
Substitute=#IconName#

[HourlyTime6]
Measure=Time
Group=Measures
TimeStamp=([HourlyTime1:Timestamp]+5*3600)
DynamicVariables=1
Format=%H

[HourlyTempValue6]
Measure=WebParser
DynamicVariables=1
URL=[OpenMeteoParser]
StringIndex=(4+[HourlyTime1:]+5)

[HourlyTemp6]
Measure=Calc
Group=Measures
Formula=Round([HourlyTempValue6],0)
Substitute="-0":"0"
DynamicVariables=1

[HourlyIcon6]
Measure=WebParser
DynamicVariables=1
URL=[OpenMeteoParser]
StringIndex=(34+[HourlyTime1:]+5)
Substitute=#IconName#

[DailyTime1]
Measure=Time
Group=Measures
FormatLocale=#Language#
DynamicVariables=1
Format=%a

[DailyLowValue1]
Measure=WebParser
URL=[OpenMeteoParser]
StringIndex=64

[DailyLow1]
Measure=Calc
Group=Measures
Formula=Round([DailyLowValue1],0)
Substitute="-0":"0"
DynamicVariables=1

[DailyHighValue1]
Measure=WebParser
URL=[OpenMeteoParser]
StringIndex=70

[DailyHigh1]
Measure=Calc
Group=Measures
Formula=Round([DailyHighValue1],0)
Substitute="-0":"0"
DynamicVariables=1

[DailyIcon1]
Measure=WebParser
URL=[OpenMeteoParser]
StringIndex=76
Substitute=#IconName#

[DailyTime2]
Measure=Time
Group=Measures
TimeStamp=([DailyTime1:TimeStamp]+86400)
FormatLocale=#Language#
DynamicVariables=1
Format=%a

[DailyLowValue2]
Measure=WebParser
URL=[OpenMeteoParser]
StringIndex=65

[DailyLow2]
Measure=Calc
Group=Measures
Formula=Round([DailyLowValue2],0)
Substitute="-0":"0"
DynamicVariables=1

[DailyHighValue2]
Measure=WebParser
URL=[OpenMeteoParser]
StringIndex=71

[DailyHigh2]
Measure=Calc
Group=Measures
Formula=Round([DailyHighValue2],0)
Substitute="-0":"0"
DynamicVariables=1

[DailyIcon2]
Measure=WebParser
Url=[OpenMeteoParser]
StringIndex=77
Substitute=#IconName#

[DailyTime3]
Measure=Time
Group=Measures
TimeStamp=([DailyTime1:TimeStamp]+2*86400)
FormatLocale=#Language#
DynamicVariables=1
Format=%a

[DailyLowValue3]
Measure=WebParser
URL=[OpenMeteoParser]
StringIndex=66

[DailyLow3]
Measure=Calc
Group=Measures
Formula=Round([DailyLowValue3],0)
Substitute="-0":"0"
DynamicVariables=1

[DailyHighValue3]
Measure=WebParser
URL=[OpenMeteoParser]
StringIndex=72

[DailyHigh3]
Measure=Calc
Group=Measures
Formula=Round([DailyHighValue3],0)
Substitute="-0":"0"
DynamicVariables=1

[DailyIcon3]
Measure=WebParser
Url=[OpenMeteoParser]
StringIndex=78
Substitute=#IconName#

[DailyTime4]
Measure=Time
Group=Measures
TimeStamp=([DailyTime1:TimeStamp]+3*86400)
FormatLocale=#Language#
DynamicVariables=1
Format=%a

[DailyLowValue4]
Measure=WebParser
URL=[OpenMeteoParser]
StringIndex=67

[DailyLow4]
Measure=Calc
Group=Measures
Formula=Round([DailyLowValue4],0)
Substitute="-0":"0"
DynamicVariables=1

[DailyHighValue4]
Measure=WebParser
URL=[OpenMeteoParser]
StringIndex=73

[DailyHigh4]
Measure=Calc
Group=Measures
Formula=Round([DailyHighValue4],0)
Substitute="-0":"0"
DynamicVariables=1

[DailyIcon4]
Measure=WebParser
Url=[OpenMeteoParser]
StringIndex=79
Substitute=#IconName#

[DailyTime5]
Measure=Time
Group=Measures
TimeStamp=([DailyTime1:TimeStamp]+4*86400)
FormatLocale=#Language#
DynamicVariables=1
Format=%a

[DailyLowValue5]
Measure=WebParser
URL=[OpenMeteoParser]
StringIndex=68

[DailyLow5]
Measure=Calc
Group=Measures
Formula=Round([DailyLowValue5],0)
Substitute="-0":"0"
DynamicVariables=1

[DailyHighValue5]
Measure=WebParser
URL=[OpenMeteoParser]
StringIndex=74

[DailyHigh5]
Measure=Calc
Group=Measures
Formula=Round([DailyHighValue5],0)
Substitute="-0":"0"
DynamicVariables=1

[DailyIcon5]
Measure=WebParser
Url=[OpenMeteoParser]
StringIndex=80
Substitute=#IconName#

[DailyTime6]
Measure=Time
Group=Measures
TimeStamp=([DailyTime1:TimeStamp]+5*86400)
FormatLocale=#Language#
DynamicVariables=1
Format=%a

[DailyLowValue6]
Measure=WebParser
URL=[OpenMeteoParser]
StringIndex=69

[DailyLow6]
Measure=Calc
Group=Measures
Formula=Round([DailyLowValue6],0)
Substitute="-0":"0"
DynamicVariables=1

[DailyHighValue6]
Measure=WebParser
URL=[OpenMeteoParser]
StringIndex=75

[DailyHigh6]
Measure=Calc
Group=Measures
Formula=Round([DailyHighValue6],0)
Substitute="-0":"0"
DynamicVariables=1

[DailyIcon6]
Measure=WebParser
Url=[OpenMeteoParser]
StringIndex=81
Substitute=#IconName#

[DailyMin]
Measure=Calc
Group=Measures
Formula=Min(Min(Min(Min(Min(([DailyLowValue1:]+0),([DailyLowValue2:]+0)),([DailyLowValue3:]+0)),([DailyLowValue4:]+0)),([DailyLowValue5:]+0)),([DailyLowValue6:]+0))
DynamicVariables=1

[DailyMax]
Measure=Calc
Group=Measures
Formula=Max(Max(Max(Max(Max(([DailyHighValue1:]+0),([DailyHighValue2:]+0)),([DailyHighValue3:]+0)),([DailyHighValue4:]+0)),([DailyHighValue5:]+0)),([DailyHighValue6:]+0))
DynamicVariables=1

[MeterStep]
Measure=Calc
Group=Measures
Formula=(#WidgetWidth# * 0.36 / ([DailyMax] - [DailyMin]))
DynamicVariables=1
