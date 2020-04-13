Name:   rpm-test-cheese
Version:  1
Release:  1
Summary:  The "cheese" package.

Group:    Facebook/script
BuildArch: noarch
License:  MIT

BuildRequires:  coreutils

%prep

%description

%build

%install
mkdir -p %{buildroot}/rpm_test/
cat >%{buildroot}/rpm_test/cheese1.txt <<EOF
This is the first cow's milk cheese text.
EOF

%clean

%files
/*
