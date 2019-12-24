Name:         toy
Version:      1.0
Release:      1
Summary:      A specfile for a toy RPM.

Group:        Facebook/script
License:      BSD
Source0:      toy_src_file
BuildArch:    noarch

%description
A very simple specfile for testing RPM builds

%prep

%build

%install
mkdir -p %{buildroot}/%{_bindir}
install -p -m 755 %{SOURCE0} %{buildroot}/%{_bindir}

%files
%{_bindir}/toy_src_file

%changelog
* Tue Sep 03 2019 Toy Maker <toy@maker.com> - 1.0-1
- Initial version of the package
