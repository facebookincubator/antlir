Name:   rpm-test-cheese
Version:  2
Release:  1
Summary:  The "cheese" package.

Group:    Facebook/script
BuildArch: x86_64, aarch64
License:  MIT

BuildRequires:  coreutils

%prep

%description

%build

%install
mkdir -p %{buildroot}/rpm_test/
cat >%{buildroot}/rpm_test/cheese2.txt <<EOF
This is the second sheep's milk cheese text.
EOF

%clean

%files
/*
