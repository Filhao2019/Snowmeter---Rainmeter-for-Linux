
ActiveNet plugin v3.1.0 by SilverAzide

  This plugin provides information about the physical CPU and battery not available from within
  Rainmeter or its standard plug-ins.
  Requires Microsoft .NET Framework 4.8 or later.

Options

  General measure options

    All general measure options are valid.

  Type

    The type of information to retrieve.

      BatChargeRate

        Returns the charge rate (in watts) for the specified battery.

      BatDeviceName

        Returns the device name of the battery.  Using the Name parameter is not required.

      BatInstanceName

        Returns the instance name of the active battery.  If multiple batteries are present and
        active, this will return the instance name of the first battery.  Using the Name parameter
        is not required.

        NOTE:  If you have the case where a system has multiple batteries and you want the measure
               to automatically "fail-over" to the next available battery once the first is
               exhausted, use DynamicVariables=1 on the measure.

      BatRemainingCapacity

        Returns the remaining capacity (in watt-hours) for the specified battery.

      BatVoltage

        Returns the voltage (in volts) for the specified battery.

        Example:

          [MeasureBatteryInstanceName]
          Measure=Plugin
          Plugin=ActiveNet
          Type=BatInstanceName
          UpdateDivider=-1

          [MeasureBatteryVoltage]
          Measure=Plugin
          Plugin=ActiveNet
          Type=BatVoltage
          Name=[MeasureBatteryInstanceName]

          In this example, the MeasureBatteryVoltage measure will return the output voltage of the
          active battery.

      CpuGroups

        Returns the total number of processor groups per machine.  Using the Name parameter is not
        required.
        
        Groups are numbered starting with 0, and will contain a maximum of 64 logical processors.
        A processor group will contain 1 or more NUMA nodes, a NUMA node will contain 1 or more
        sockets, a socket will contain 1 or more physical cores, and a physical core will contain 1
        or 2 logical processors.  A machine that is not running in NUMA mode (UMA mode) will have 1
        processor group containing 1 NUMA node with up to 64 logical processors (any logical
        processors in excess of 64 will not be available or visible to Windows in this case).

      CpuLogicalCoreMap

        Returns a mapping of all logical cores to all physical cores as a string of comma-separated
        values.  The numeric value will be the total number of logical processors (for all CPUs),
        which will match the number of values in the string.  Using the Name parameter is not
        required.

        Examples:
          A single-socket 4C/8T processor will return a string value of "0,0,1,1,2,2,3,3", and a
            numeric value of 8.
          Dual-socket 4C/8T processors will return a string value of
            "0,0,1,1,2,2,3,3,4,4,5,5,6,6,7,7" and a numeric value of 16.
          A hybrid 10C/16T processor (6 hyperthreaded cores + 4 single-threaded cores) will return
            a string value of "0,0,1,1,2,2,3,3,4,4,5,5,6,7,8,9" (or possibly other physical
            arrangements), and a numeric value of 16.

      CpuLogicalCores

        Returns the number of logical cores per physical CPU.  Using the Name parameter is not
        required.

      CpuNumaNodes

        Returns the total number of NUMA nodes across all processor groups when running in NUMA
        mode.  If the processor(s) are not running in NUMA mode, then this will return the same
        number as CpuProcessors.  Nodes are numbered starting with 0.  Using the Name parameter is
        not required.

      CpuPhysicalCores

        Returns the number of physical cores per physical CPU.  Using the Name parameter is not
        required.

      CpuProcessors

        Returns the number of physical CPUs/sockets/packages.  Using the Name parameter is not
        required.

      CpuThreadsPerCore

        OBSOLETE, DO NOT USE
        Returns the number of threads per physical core.  Using the Name parameter is not required.
        Possible return values are:

           1 = for single-threaded CPUs,
           2 = for hyperthreaded CPUs,
          -1 = for hybrid CPUs; this option is not applicable for this type of CPU.  Hybrid CPUs
               have both single-threaded cores and hyperthreaded cores.

        Alternatively, this value can be calculated:
          CpuThreadsPerCore = CpuLogicalCores / CpuPhysicalCores.

  Name

    For battery measures, this option is the instance name of the battery for which information
    will be retrieved; e.g., "ACPI\PNP0C0A\0_0".  Do not confuse the instance name with the device
    name.  Laptops with multiple batteries (or the option for multiple batteries), will have
    multiple instance names.  To see detailed information about each of the batteries in your
    system, at a PowerShell prompt enter the command:

      PS C:\> gwmi -Class BatteryStatus -Namespace root\wmi

    This option is not used for CPU measures.


History:
  3.1.0  - 2022-01-10:  Corrected logic to work properly with machines having multiple processor
                        groups.
                        Added CpuGroups to return the number of processor groups per machine.
  3.0.0  - 2021-09-11:  Removed all network adapter options (now obsolete, use Rainmeter's SysInfo).
  2.13.0 - 2021-08-21:  Added CpuLogicalCores to return the number of logical cores per CPU.
                        Added CpuLogicalCoreMap to return logical-to-physical core mapping.
                        Revised CpuThreadsPerCore for hybrid CPUs; this option is obsolete.
  2.12.0 - 2021-05-22:  Added CpuNumaNodes type to return the total number of NUMA nodes for
                          processors running in NUMA mode.
  2.11.0 - 2021-05-14:  Updated to .NET Framework 4.8.
                        Revised CpuProcessors type to improve method of counting physical CPUs.
  2.10.0 - 2020-07-27:  Added NetSpeed type to return the maximum speed of the network interface in
                          bits per second.
  2.9.0  - 2019-05-05:  Added NetStatus type to retrieve a network adapter's operational status.
  2.8.0  - 2018-07-12:  Corrected CpuThreadsPerCore type on machines with multiple physical CPUs.
  2.7.0  - 2017-10-07:  Updated to latest Rainmeter SDK.
  2.6.1  - 2017-09-22:  Updated to latest Rainmeter SDK and Visual Studio 2017.
  2.6.0  - 2017-07-06:  Removed NetAdapterTypeID type as it is not needed (use NetAdapterType).
  2.5.0  - 2016-06-07:  Added battery types BatChargeRate, BatDeviceName, BatInstanceName,
                          BatRemainingCapacity, BatVoltage.
                        Renamed CpuPhysicalCores, CpuProcessors, CpuThreadsPerCore (types
                          PhysicalCores, Processors, ThreadsPerCore are obsolete).
                        Renamed NetAdapterType, NetAdapterTypeID, NetInterfaceID, NetInterfaceName
                          (types AdapterType, AdapterTypeID, InterfaceID, InterfaceName are obsolete).
  2.4.0  - 2015-10-09:  Complete rewrite due to Rainmeter SysInfo plugin enhancements.  This plugin
                          no longer will retrieve the active adapter; this is now done using
                          Rainmeter's new "SysInfoData=Best/<adapter name>" feature.
                        Removed AdapterName, IPv4Address types (now obsolete, use Rainmeter).
                        Removed Index option; use Name option instead as Index is unreliable and
                          should never be used in any scenario.
  2.3.0  - 2015-09-10:  Added CPU types PhysicalCores, Processors, ThreadsPerCore.
  2.1.0  - 2015-08-23:  Corrected network adapter changes added in v1.3.0.
  1.3.0  - 2015-07-03:  Removed AdapterCount type (no longer needed).
                        Removed Format type.
                        Added Name option to allow specifying adapter by name instead of index.
                        Revised code to retrieve network adapter data in the same manner as
                          Rainmeter (Win32 API).
  1.2.0  - 2015-05-09:  Added Format option to return fixed precision formatted numbers.
  1.1.0  - 2015-04-27:  Revised InterfaceID type to return the same interface number as Rainmeter.
  1.0.0  - 2015-04-25:  Initial release.
